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
  const cwd = normalizeFileName(path.posix.resolve(program.cwd ?? "/"));
  const files = new Map();
  const publicNames = new Map();

  for (const lib of program.libs ?? []) {
    const publicName = normalizeFileName(lib);
    const fileName = absoluteProgramFileName(publicName, cwd);
    const libPath = path.join(vendorLibDir, publicName);
    files.set(fileName, fs.readFileSync(libPath, "utf8"));
    publicNames.set(fileName, publicName);
  }

  for (const file of program.files ?? []) {
    const publicName = normalizeFileName(file.name);
    const fileName = absoluteProgramFileName(publicName, cwd);
    files.set(fileName, Buffer.from(file.textB64, "base64").toString("utf8"));
    publicNames.set(fileName, publicName);
  }

  return { program, cwd, files, publicNames };
}

function normalizeFileName(fileName) {
  return fileName.replace(/\\/g, "/");
}

function absoluteProgramFileName(fileName, cwd) {
  const normalized = normalizeFileName(fileName);
  return path.posix.isAbsolute(normalized) ? normalized : normalizeFileName(path.posix.resolve(cwd, normalized));
}

function compilerOptionsFromProgram(program) {
  const declarations = new Map(
    (ts.optionDeclarations ?? []).map((option) => [option.name.toLowerCase(), option])
  );
  const options = {};

  for (const [rawName, rawValue] of Object.entries(program.options ?? {})) {
    const declaration = declarations.get(rawName.toLowerCase());
    const name = declaration?.name ?? rawName;
    if (declaration?.type instanceof Map && typeof rawValue === "string") {
      const value = declaration.type.get(rawValue.toLowerCase());
      if (value === undefined) {
        throw new Error(
          `Argument for '--${name}' option must be: ${Array.from(declaration.type.keys()).join(", ")}.`
        );
      }
      options[name] = value;
    } else {
      options[name] = rawValue;
    }
  }

  return {
    ...options,
    noLib: true,
  };
}

function createHost(options, files, cwd) {
  const languageVersion = options.target ?? ts.ScriptTarget.Latest;
  return {
    getSourceFile(fileName) {
      const normalized = absoluteProgramFileName(fileName, cwd);
      const text = files.get(normalized);
      return text === undefined
        ? undefined
        : ts.createSourceFile(normalized, text, languageVersion, true);
    },
    getDefaultLibFileName() {
      return "lib.d.ts";
    },
    getCurrentDirectory() {
      return cwd;
    },
    getDirectories() {
      return [];
    },
    getCanonicalFileName(fileName) {
      return absoluteProgramFileName(fileName, cwd);
    },
    useCaseSensitiveFileNames() {
      return true;
    },
    getNewLine() {
      return "\n";
    },
    fileExists(fileName) {
      return files.has(absoluteProgramFileName(fileName, cwd));
    },
    readFile(fileName) {
      return files.get(absoluteProgramFileName(fileName, cwd));
    },
    writeFile() {},
  };
}

function collectDiagnostics(programJsonPath) {
  const { program: programJson, cwd, files, publicNames } = decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map((fileName) => absoluteProgramFileName(fileName, cwd)),
    ...(programJson.files ?? []).map((file) => absoluteProgramFileName(file.name, cwd)),
  ];
  const host = createHost(options, files, cwd);
  const program = ts.createProgram(rootNames, options, host);
  const diagnostics = [];

  for (const file of programJson.files ?? []) {
    const sourceFile = program.getSourceFile(absoluteProgramFileName(file.name, cwd));
    if (!sourceFile) continue;
    diagnostics.push(...program.getSyntacticDiagnostics(sourceFile));
    diagnostics.push(...program.getSemanticDiagnostics(sourceFile));
    diagnostics.push(...program.getSuggestionDiagnostics(sourceFile));
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

function publicFileName(file, publicNames) {
  if (!file) return null;
  return publicNames.get(normalizeFileName(file.fileName)) ?? normalizeFileName(file.fileName);
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
