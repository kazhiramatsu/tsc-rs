import inspector from "node:inspector";
import fs from "node:fs";
import readline from "node:readline";
import { createRequire } from "node:module";
import {
  absoluteProgramFileName,
  categoryNames,
  decodeProgram,
  publicFileName,
} from "./program-host.mjs";

const [instrumentedPath, inventoryPath, rawMaxLibBuckets] = process.argv.slice(2);
if (!instrumentedPath || !inventoryPath) {
  throw new Error(
    "usage: trace-driver.mjs <instrumented-_tsc.cjs> <m8-emitter-inventory.json> [max-lib-cache-buckets]",
  );
}
const require = createRequire(import.meta.url);
const ts = require(instrumentedPath);
const inventory = JSON.parse(await fs.promises.readFile(inventoryPath, "utf8"));
const instrumentedText = await fs.promises.readFile(instrumentedPath, "utf8");
if (inventory.schema !== 2 || inventory.band !== "all") {
  throw new Error("diagnostic trace driver requires the schema-2 all-band D2 inventory");
}
for (const name of [
  "__tsrsM8ResetTrace",
  "__tsrsM8SetTracePass",
  "__tsrsM8TakeTrace",
]) {
  if (typeof ts[name] !== "function") {
    throw new Error(`instrumented oracle does not export ${name}`);
  }
}
const maxLibSourceFileBuckets = Number(rawMaxLibBuckets ?? "8");
if (
  !Number.isSafeInteger(maxLibSourceFileBuckets) ||
  maxLibSourceFileBuckets < 1
) {
  throw new Error("max-lib-cache-buckets must be a positive integer");
}

const declarations = inventory.functions
  .map((declaration) => ({
    id: declaration.id,
    start: declaration.source_range.start.offset,
    end: declaration.source_range.end.offset,
  }))
  .sort((left, right) => left.start - right.start || right.end - left.end);
const declarationByStart = new Map(
  declarations.map((declaration) => [declaration.start, declaration]),
);
const generatedLineStarts = [0];
for (let offset = 0; offset < instrumentedText.length; offset += 1) {
  if (instrumentedText.charCodeAt(offset) === 10) {
    generatedLineStarts.push(offset + 1);
  }
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
  const optionDeclarations = new Map(
    (ts.optionDeclarations ?? []).map((option) => [option.name.toLowerCase(), option]),
  );
  const options = {};
  for (const [rawName, rawValue] of Object.entries(program.options ?? {})) {
    const declaration = optionDeclarations.get(rawName.toLowerCase());
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
  const { program: programJson, cwd, files, publicNames } =
    decodeProgram(programJsonPath);
  const options = compilerOptionsFromProgram(programJson);
  const rootNames = [
    ...(programJson.libs ?? []).map((fileName) =>
      absoluteProgramFileName(fileName, cwd),
    ),
    ...(programJson.files ?? []).map((file) =>
      absoluteProgramFileName(file.name, cwd),
    ),
  ];
  const libNames = new Set(
    (programJson.libs ?? []).map((fileName) =>
      absoluteProgramFileName(fileName, cwd),
    ),
  );
  const host = createHost(options, files, cwd, libNames);
  const program = ts.createProgram(rootNames, options, host);
  return { program, programJson, cwd, publicNames };
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

function collectDiagnostics(programJsonPath) {
  ts.__tsrsM8ResetTrace();
  ts.__tsrsM8SetTracePass("syntactic");
  const { program, programJson, cwd, publicNames } =
    createProgramFromJsonPath(programJsonPath);
  const diagnostics = [];
  for (const file of programJson.files ?? []) {
    const fileName = file.name.replace(/\\/g, "/");
    const absolute = fileName.startsWith("/")
      ? fileName
      : `${cwd}/${fileName}`.replace(/\/+/g, "/");
    const sourceFile = program.getSourceFile(absolute);
    if (!sourceFile) continue;
    for (const [pass, getPassDiagnostics] of [
      ["syntactic", () => program.getSyntacticDiagnostics(sourceFile)],
      ["semantic", () => program.getSemanticDiagnostics(sourceFile)],
      ["suggestion", () => program.getSuggestionDiagnostics(sourceFile)],
    ]) {
      ts.__tsrsM8SetTracePass(pass);
      const passDiagnostics = getPassDiagnostics();
      for (const diagnostic of passDiagnostics) {
        diagnostic.tsrsPass ??= pass;
        diagnostics.push(diagnostic);
      }
    }
  }
  ts.__tsrsM8SetTracePass(null);
  return {
    diagnostics: ts
      .sortAndDeduplicateDiagnostics(diagnostics)
      .map((diagnostic) => serializeDiagnostic(diagnostic, publicNames)),
    trace: ts.__tsrsM8TakeTrace(),
  };
}

function post(session, method, params = {}) {
  return new Promise((resolve, reject) => {
    session.post(method, params, (error, result) => {
      if (error) reject(error);
      else resolve(result);
    });
  });
}

function inverseOffset(generatedOffset, offsetMap) {
  let delta = 0;
  for (const edit of offsetMap) {
    if (generatedOffset < edit.generated_start) {
      return generatedOffset - delta;
    }
    if (generatedOffset < edit.generated_end) {
      return edit.original_start;
    }
    delta +=
      edit.generated_end -
      edit.generated_start -
      (edit.original_end - edit.original_start);
  }
  return generatedOffset - delta;
}

function declarationContainingOffset(offset) {
  let selected = null;
  for (const declaration of declarations) {
    if (declaration.start > offset) break;
    if (
      offset < declaration.end &&
      (!selected ||
        declaration.start > selected.start ||
        (declaration.start === selected.start && declaration.end < selected.end))
    ) {
      selected = declaration;
    }
  }
  return selected;
}

function exactTrace(events, offsetMap) {
  return events.map((event) => ({
    ...event,
    frames: event.frames.map((frame) => {
      if (
        frame.file !== instrumentedPath ||
        !Number.isSafeInteger(frame.line) ||
        !Number.isSafeInteger(frame.column) ||
        frame.line < 1 ||
        frame.line > generatedLineStarts.length ||
        frame.column < 1
      ) {
        return {
          ...frame,
          original_offset: null,
          d2_declaration: null,
        };
      }
      const generatedOffset =
        generatedLineStarts[frame.line - 1] + frame.column - 1;
      const originalOffset = inverseOffset(generatedOffset, offsetMap);
      return {
        ...frame,
        original_offset: originalOffset,
        d2_declaration:
          declarationContainingOffset(originalOffset)?.id ?? null,
      };
    }),
  }));
}

function exactCoverage(coverageResult, offsetMap) {
  const script = coverageResult.result.find(
    (entry) =>
      entry.url === instrumentedPath ||
      entry.url.endsWith(`/${instrumentedPath.split("/").at(-1)}`),
  );
  if (!script) {
    throw new Error("precise coverage did not return the instrumented _tsc.js script");
  }
  const exact = new Set();
  let unmappedCount = 0;
  const unmappedSample = [];
  for (const fn of script.functions) {
    if (!fn.ranges.some((range) => range.count > 0)) continue;
    const range = fn.ranges[0];
    const originalStart = inverseOffset(range.startOffset, offsetMap);
    const declaration = declarationByStart.get(originalStart);
    if (declaration) {
      exact.add(declaration.id);
    } else {
      unmappedCount += 1;
      if (unmappedSample.length < 20) {
        unmappedSample.push({
          function_name: fn.functionName || null,
          generated_start: range.startOffset,
          generated_end: range.endOffset,
          original_start: originalStart,
        });
      }
    }
  }
  return {
    exact_d2_declarations: [...exact].sort(),
    unmapped_function_ranges: unmappedCount,
    unmapped_sample: unmappedSample,
  };
}

const offsetMap = JSON.parse(process.env.TSRS_M8_TRACE_OFFSET_MAP ?? "null");
if (!Array.isArray(offsetMap)) {
  throw new Error("TSRS_M8_TRACE_OFFSET_MAP must contain the instrumentation offset map");
}
const session = new inspector.Session();
session.connect();
await post(session, "Profiler.enable");

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  // Per-probe coverage is a comparison input, so a sibling may not
  // inherit parsed lib SourceFiles from whichever probe ran first.
  // Keep caching bounded within one program and reset it at the probe
  // boundary.
  libSourceFileBuckets.clear();
  await post(session, "Profiler.startPreciseCoverage", {
    callCount: true,
    detailed: false,
  });
  const result = collectDiagnostics(request.programJsonPath);
  const coverageResult = await post(session, "Profiler.takePreciseCoverage");
  await post(session, "Profiler.stopPreciseCoverage");
  process.stdout.write(
    `${JSON.stringify({
      id: request.id ?? null,
      diagnostics: result.diagnostics,
      trace: exactTrace(result.trace, offsetMap),
      coverage: exactCoverage(coverageResult, offsetMap),
    })}\n`,
  );
}
session.disconnect();
