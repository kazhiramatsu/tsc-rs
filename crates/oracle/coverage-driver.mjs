import readline from "node:readline";
import { createRequire } from "node:module";
import {
  absoluteProgramFileName,
  categoryNames,
  decodeProgram,
  publicFileName,
} from "./program-host.mjs";

const instrumentedPath = process.argv[2];
if (!instrumentedPath) {
  throw new Error(
    "usage: coverage-driver.mjs <instrumented-_tsc.cjs> [max-lib-cache-buckets]",
  );
}
const require = createRequire(import.meta.url);
const ts = require(instrumentedPath);
const maxLibSourceFileBuckets = Number(process.argv[3] ?? "8");
if (
  !Number.isSafeInteger(maxLibSourceFileBuckets) ||
  maxLibSourceFileBuckets < 1
) {
  throw new Error("max-lib-cache-buckets must be a positive integer");
}
const libSourceFileBuckets = new Map();

function stableObjectKey(value) {
  return JSON.stringify(
    Object.fromEntries(
      Object.entries(value)
        .filter(([, entry]) => typeof entry !== "function")
        .sort(([left], [right]) => left.localeCompare(right)),
    ),
  );
}

function libSourceFileBucket(key) {
  const cached = libSourceFileBuckets.get(key);
  if (cached) {
    libSourceFileBuckets.delete(key);
    libSourceFileBuckets.set(key, cached);
    return cached;
  }
  const created = new Map();
  libSourceFileBuckets.set(key, created);
  if (libSourceFileBuckets.size > maxLibSourceFileBuckets) {
    libSourceFileBuckets.delete(libSourceFileBuckets.keys().next().value);
  }
  return created;
}

function compilerOptionsFromProgram(program) {
  const declarations = new Map(
    (ts.optionDeclarations ?? []).map((option) => [option.name.toLowerCase(), option]),
  );
  const options = {};
  for (const [rawName, rawValue] of Object.entries(program.options ?? {})) {
    const declaration = declarations.get(rawName.toLowerCase());
    const name = declaration?.name ?? rawName;
    if (declaration?.type instanceof Map && typeof rawValue === "string") {
      const value = declaration.type.get(rawValue.toLowerCase());
      if (value === undefined) {
        throw new Error(
          `Argument for '--${name}' option must be: ${Array.from(declaration.type.keys()).join(", ")}.`,
        );
      }
      options[name] = value;
    } else {
      options[name] = rawValue;
    }
  }
  return { ...options, noLib: true };
}

function createHost(options, files, cwd, libNames) {
  const languageVersion = options.target ?? ts.ScriptTarget.Latest;
  const optionKey = ts.getKeyForCompilerOptions(
    options,
    ts.sourceFileAffectingCompilerOptions,
  );
  return {
    getSourceFile(fileName, languageVersionOrOptions = languageVersion) {
      const normalized = absoluteProgramFileName(fileName, cwd);
      const text = files.get(normalized);
      if (text === undefined) return undefined;
      if (!libNames.has(normalized)) {
        return ts.createSourceFile(normalized, text, languageVersionOrOptions, true);
      }
      const cacheKey = `${optionKey}\0${stableObjectKey(
        typeof languageVersionOrOptions === "object"
          ? languageVersionOrOptions
          : { languageVersion: languageVersionOrOptions },
      )}`;
      const bucket = libSourceFileBucket(cacheKey);
      let sourceFile = bucket.get(normalized);
      if (!sourceFile) {
        sourceFile = ts.createSourceFile(
          normalized,
          text,
          languageVersionOrOptions,
          true,
        );
        bucket.set(normalized, sourceFile);
      }
      return sourceFile;
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

function createProgramFromJsonPath(programJsonPath) {
  const { program: programJson, cwd, files, publicNames } = decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map((fileName) => absoluteProgramFileName(fileName, cwd)),
    ...(programJson.files ?? []).map((file) => absoluteProgramFileName(file.name, cwd)),
  ];
  const libNames = new Set(
    (programJson.libs ?? []).map((fileName) => absoluteProgramFileName(fileName, cwd)),
  );
  const host = createHost(options, files, cwd, libNames);
  const program = ts.createProgram(rootNames, options, host);
  return { program, programJson, cwd, publicNames };
}

function exercise(programJsonPath, collectDiagnostics) {
  const { program, programJson, cwd, publicNames } =
    createProgramFromJsonPath(programJsonPath);
  const diagnostics = [];
  for (const file of programJson.files ?? []) {
    const fileName = file.name.replace(/\\/g, "/");
    const absolute = fileName.startsWith("/") ? fileName : `${cwd}/${fileName}`.replace(/\/+/g, "/");
    const sourceFile = program.getSourceFile(absolute);
    if (!sourceFile) continue;
    for (const [pass, passDiagnostics] of [
      ["syntactic", program.getSyntacticDiagnostics(sourceFile)],
      ["semantic", program.getSemanticDiagnostics(sourceFile)],
      ["suggestion", program.getSuggestionDiagnostics(sourceFile)],
    ]) {
      if (collectDiagnostics) {
        for (const diagnostic of passDiagnostics) {
          diagnostic.tsrsPass ??= pass;
          diagnostics.push(diagnostic);
        }
      }
    }
  }
  return collectDiagnostics
    ? ts
        .sortAndDeduplicateDiagnostics(diagnostics)
        .map((diagnostic) => serializeDiagnostic(diagnostic, publicNames))
    : undefined;
}

function serializeDiagnostic(diagnostic, publicNames) {
  return {
    file: publicFileName(diagnostic.file, publicNames),
    start: diagnostic.start ?? null,
    length: diagnostic.length ?? null,
    code: diagnostic.code,
    pass: diagnostic.tsrsPass ?? null,
    category: categoryNames[diagnostic.category] ?? "message",
    chain: serializeMessageText(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
    ),
    related: (diagnostic.relatedInformation ?? []).map((related) =>
      serializeRelated(related, publicNames),
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
    chain: serializeMessageText(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
    ),
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
      serializeMessageText(next, next.code, next.category),
    ),
  };
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  if (request.finish) {
    if (
      !Array.isArray(ts.__tsrsM8HitIds) ||
      !(ts.__tsrsM8Hits instanceof Uint8Array) ||
      ts.__tsrsM8HitIds.length !== ts.__tsrsM8Hits.length
    ) {
      throw new Error("instrumented oracle returned an invalid emitter hit-set");
    }
    const counts = Object.create(null);
    for (let index = 0; index < ts.__tsrsM8Hits.length; index += 1) {
      if (ts.__tsrsM8Hits[index] !== 0) {
        counts[ts.__tsrsM8HitIds[index]] = 1;
      }
    }
    process.stdout.write(
      `${JSON.stringify({
        schema: 1,
        counts,
        cache: {
          buckets: libSourceFileBuckets.size,
          sourceFiles: Array.from(libSourceFileBuckets.values()).reduce(
            (total, bucket) => total + bucket.size,
            0,
          ),
        },
      })}\n`,
    );
    break;
  }
  const diagnostics = exercise(
    request.programJsonPath,
    request.returnDiagnostics === true,
  );
  if (diagnostics) {
    process.stdout.write(
      `${JSON.stringify({ id: request.id ?? null, diagnostics })}\n`,
    );
  }
}
