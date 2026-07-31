import ts from "../../vendor/typescript-6.0.3/lib/typescript.js";
import {
  M9RequestError,
  publicFileNameForSource,
} from "./m9-program-host.mjs";

export const MAX_MESSAGE_CHAIN_DEPTH = 32;
export const MAX_MESSAGE_CHAIN_NODES = 4_096;

const U32_MAX = 0xffff_ffff;
const categoryNames = ["warning", "error", "suggestion", "message"];

export class M9ObservationError extends Error {
  constructor(message) {
    super(message);
    this.name = "M9ObservationError";
  }
}

function observationError(error) {
  if (error instanceof M9ObservationError) return error;
  return new M9ObservationError(String(error?.message ?? error));
}

function assertU32(value, context) {
  if (!Number.isInteger(value) || value < 0 || value > U32_MAX) {
    throw new M9ObservationError(`${context} must be a u32`);
  }
  return value;
}

function categoryName(value, context) {
  if (
    !Number.isInteger(value) ||
    value < 0 ||
    value >= categoryNames.length
  ) {
    throw new M9ObservationError(
      `${context} has unknown diagnostic category ${JSON.stringify(value)}`,
    );
  }
  return categoryNames[value];
}

function optionalU32(value, context) {
  if (value === undefined) return { kind: "absent" };
  return { kind: "present", value: assertU32(value, context) };
}

function optionalBoolean(owner, property, context) {
  const value = owner[property];
  if (value === undefined) return { present: false, value: null };
  if (typeof value !== "boolean") {
    throw new M9ObservationError(`${context} must be boolean when present`);
  }
  return { present: true, value };
}

function optionalString(owner, property, context) {
  const value = owner[property];
  if (value === undefined) return { present: false, value: null };
  if (typeof value !== "string") {
    throw new M9ObservationError(`${context} must be a string when present`);
  }
  return { present: true, value };
}

function inspectMessageNode(
  input,
  fallbackCode,
  fallbackCategory,
  depth,
  nodeState,
  context,
) {
  if (depth > MAX_MESSAGE_CHAIN_DEPTH) {
    throw new M9ObservationError(
      `${context} exceeds message-chain depth ${MAX_MESSAGE_CHAIN_DEPTH}`,
    );
  }
  nodeState.count += 1;
  if (nodeState.count > MAX_MESSAGE_CHAIN_NODES) {
    throw new M9ObservationError(
      `${context} exceeds message-chain node count ${MAX_MESSAGE_CHAIN_NODES}`,
    );
  }

  if (typeof input === "string") {
    if (input.length === 0) {
      throw new M9ObservationError(`${context}.text must not be empty`);
    }
    return {
      input: null,
      children: [],
      output: {
        text: input,
        code: assertU32(fallbackCode, `${context}.code`),
        category: categoryName(
          fallbackCategory,
          `${context}.category`,
        ),
        next_present: false,
        next: [],
      },
    };
  }

  if (input === null || typeof input !== "object" || Array.isArray(input)) {
    throw new M9ObservationError(
      `${context} must be a string or message-chain object`,
    );
  }
  if (typeof input.messageText !== "string" || input.messageText.length === 0) {
    throw new M9ObservationError(
      `${context}.messageText must be a non-empty string`,
    );
  }
  const nextPresent = input.next !== undefined;
  if (nextPresent && !Array.isArray(input.next)) {
    throw new M9ObservationError(
      `${context}.next must be an array when present`,
    );
  }
  return {
    input,
    children: nextPresent ? input.next : [],
    output: {
      text: input.messageText,
      code: assertU32(input.code, `${context}.code`),
      category: categoryName(input.category, `${context}.category`),
      next_present: nextPresent,
      next: [],
    },
  };
}

export function serializeMessageChainBounded(
  messageText,
  fallbackCode,
  fallbackCategory,
  context = "diagnostic.chain",
) {
  const nodeState = { count: 0 };
  const root = inspectMessageNode(
    messageText,
    fallbackCode,
    fallbackCategory,
    1,
    nodeState,
    context,
  );
  if (root.input === null) return root.output;

  const activeAncestors = new Set([root.input]);
  const stack = [
    {
      input: root.input,
      output: root.output,
      children: root.children,
      nextIndex: 0,
      depth: 1,
      context,
    },
  ];

  while (stack.length > 0) {
    const frame = stack[stack.length - 1];
    if (frame.nextIndex === frame.children.length) {
      activeAncestors.delete(frame.input);
      stack.pop();
      continue;
    }

    const childIndex = frame.nextIndex;
    frame.nextIndex += 1;
    const childInput = frame.children[childIndex];
    const childContext = `${frame.context}.next[${childIndex}]`;
    if (
      childInput === null ||
      typeof childInput !== "object" ||
      Array.isArray(childInput)
    ) {
      throw new M9ObservationError(
        `${childContext} must be a message-chain object`,
      );
    }
    if (activeAncestors.has(childInput)) {
      throw new M9ObservationError(
        `${childContext} contains an active-ancestor cycle`,
      );
    }
    const child = inspectMessageNode(
      childInput,
      undefined,
      undefined,
      frame.depth + 1,
      nodeState,
      childContext,
    );
    frame.output.next.push(child.output);
    activeAncestors.add(child.input);
    stack.push({
      input: child.input,
      output: child.output,
      children: child.children,
      nextIndex: 0,
      depth: frame.depth + 1,
      context: childContext,
    });
  }

  return root.output;
}

export function snapshotCanonicalHead(
  diagnostic,
  context = "diagnostic.canonicalHead",
) {
  const canonicalHead = diagnostic.canonicalHead;
  if (canonicalHead === undefined) return { kind: "absent" };
  if (
    canonicalHead === null ||
    typeof canonicalHead !== "object" ||
    Array.isArray(canonicalHead)
  ) {
    throw new M9ObservationError(`${context} must be an object`);
  }
  const code = assertU32(canonicalHead.code, `${context}.code`);
  if (code === 0) {
    throw new M9ObservationError(`${context}.code must be non-zero`);
  }
  if (
    typeof canonicalHead.messageText !== "string" ||
    canonicalHead.messageText.length === 0
  ) {
    throw new M9ObservationError(
      `${context}.messageText must be a non-empty string`,
    );
  }
  return {
    kind: "present",
    code,
    message_text: canonicalHead.messageText,
  };
}

function publicFileName(file, publicNames, context) {
  try {
    return publicFileNameForSource(file, publicNames);
  } catch (error) {
    if (error instanceof M9RequestError) {
      throw new M9ObservationError(`${context}: ${error.message}`);
    }
    throw error;
  }
}

function serializeRelated(diagnostic, publicNames, context) {
  if (
    diagnostic === null ||
    typeof diagnostic !== "object" ||
    Array.isArray(diagnostic)
  ) {
    throw new M9ObservationError(`${context} must be a diagnostic object`);
  }
  const file = publicFileName(
    diagnostic.file,
    publicNames,
    `${context}.file`,
  );
  const start = diagnostic.start;
  const length = diagnostic.length;
  if ((start === undefined) !== (length === undefined)) {
    throw new M9ObservationError(
      `${context}.start and length presence must match`,
    );
  }
  if (file === null && start !== undefined) {
    throw new M9ObservationError(
      `${context} without a file must not carry a span`,
    );
  }
  return {
    file_present: file !== null,
    file,
    start_present: start !== undefined,
    start:
      start === undefined ? null : assertU32(start, `${context}.start`),
    length_present: length !== undefined,
    length:
      length === undefined
        ? null
        : assertU32(length, `${context}.length`),
    code: assertU32(diagnostic.code, `${context}.code`),
    category: categoryName(
      diagnostic.category,
      `${context}.category`,
    ),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
  };
}

export function serializeDiagnosticOccurrence(
  diagnostic,
  pass,
  publicNames,
  context = "diagnostic",
) {
  if (
    diagnostic === null ||
    typeof diagnostic !== "object" ||
    Array.isArray(diagnostic)
  ) {
    throw new M9ObservationError(`${context} must be a diagnostic object`);
  }
  if (!["syntactic", "semantic", "suggestion"].includes(pass)) {
    throw new M9ObservationError(`${context}.pass is unknown`);
  }

  const file = publicFileName(
    diagnostic.file,
    publicNames,
    `${context}.file`,
  );
  const start = diagnostic.start;
  const length = diagnostic.length;
  if ((start === undefined) !== (length === undefined)) {
    throw new M9ObservationError(
      `${context}.start and length presence must match`,
    );
  }
  if (file === null && start !== undefined) {
    throw new M9ObservationError(
      `${context} global diagnostic must not carry a span`,
    );
  }

  let line = { kind: "absent" };
  let column = { kind: "absent" };
  if (file !== null && start !== undefined) {
    if (
      !diagnostic.file ||
      typeof diagnostic.file.getLineAndCharacterOfPosition !== "function"
    ) {
      throw new M9ObservationError(
        `${context}.file cannot translate a diagnostic position`,
      );
    }
    const location =
      diagnostic.file.getLineAndCharacterOfPosition(start);
    line = optionalU32(location?.line, `${context}.line`);
    column = optionalU32(location?.character, `${context}.column`);
  }

  const relatedInformation = diagnostic.relatedInformation;
  if (
    relatedInformation !== undefined &&
    !Array.isArray(relatedInformation)
  ) {
    throw new M9ObservationError(
      `${context}.relatedInformation must be an array when present`,
    );
  }

  return {
    pass,
    file:
      file === null
        ? { kind: "global" }
        : { kind: "file", path: file },
    code: assertU32(diagnostic.code, `${context}.code`),
    line,
    column,
    category: categoryName(
      diagnostic.category,
      `${context}.category`,
    ),
    start: optionalU32(start, `${context}.start`),
    length: optionalU32(length, `${context}.length`),
    chain: serializeMessageChainBounded(
      diagnostic.messageText,
      diagnostic.code,
      diagnostic.category,
      `${context}.chain`,
    ),
    related_information_present: relatedInformation !== undefined,
    related: (relatedInformation ?? []).map((related, index) =>
      serializeRelated(
        related,
        publicNames,
        `${context}.related[${index}]`,
      ),
    ),
    reports_unnecessary: optionalBoolean(
      diagnostic,
      "reportsUnnecessary",
      `${context}.reportsUnnecessary`,
    ),
    reports_deprecated: optionalBoolean(
      diagnostic,
      "reportsDeprecated",
      `${context}.reportsDeprecated`,
    ),
    source: optionalString(
      diagnostic,
      "source",
      `${context}.source`,
    ),
  };
}

export function collectPassOccurrences(programState) {
  const { program, files, publicNames } = programState;
  const occurrences = [];
  for (const file of files) {
    const sourceFile = program.getSourceFile(file.resolvedName);
    // Host-only files and roots rejected by tsc's extension/allowJs
    // gate remain exact CaseSpec inputs but have no public diagnostic
    // getter. This is the same continue used by the production oracle.
    if (!sourceFile) continue;
    const passes = [
      ["syntactic", () => program.getSyntacticDiagnostics(sourceFile)],
      ["semantic", () => program.getSemanticDiagnostics(sourceFile)],
      ["suggestion", () => program.getSuggestionDiagnostics(sourceFile)],
    ];
    for (const [pass, getDiagnostics] of passes) {
      // Snapshot every occurrence before invoking the next public getter.
      // A diagnostic object may be reused by tsc across passes.
      const diagnostics = getDiagnostics();
      if (!Array.isArray(diagnostics)) {
        throw new M9ObservationError(
          `${file.publicName} ${pass} getter did not return an array`,
        );
      }
      for (const diagnostic of diagnostics) {
        const index = occurrences.length;
        try {
          occurrences.push({
            diagnosticObject: diagnostic,
            assembled: {
              diagnostic: serializeDiagnosticOccurrence(
                diagnostic,
                pass,
                publicNames,
                `assembled[${index}].diagnostic`,
              ),
              // Snapshot before the downstream global sort/dedupe.
              canonical_head: snapshotCanonicalHead(
                diagnostic,
                `assembled[${index}].canonical_head`,
              ),
            },
          });
        } catch (error) {
          throw observationError(error);
        }
      }
    }
  }
  return occurrences;
}

function stripAnsi(text) {
  return text.replace(/\x1B\[[0-9;]*m/gu, "");
}

export function formatRawDiagnostic(diagnostic, cwd) {
  const host = {
    getCurrentDirectory() {
      return cwd;
    },
    getCanonicalFileName(fileName) {
      return fileName.replaceAll("\\", "/");
    },
    getNewLine() {
      return "\n";
    },
  };

  let formatted;
  if (diagnostic.category === ts.DiagnosticCategory.Suggestion) {
    const messageDiagnostic = {
      ...diagnostic,
      category: ts.DiagnosticCategory.Message,
    };
    const messageName =
      ts.DiagnosticCategory[ts.DiagnosticCategory.Message];
    try {
      ts.DiagnosticCategory[ts.DiagnosticCategory.Message] =
        "Suggestion";
      formatted = ts.formatDiagnosticsWithColorAndContext(
        [messageDiagnostic],
        host,
      );
    } finally {
      ts.DiagnosticCategory[ts.DiagnosticCategory.Message] =
        messageName;
    }
  } else {
    formatted = ts.formatDiagnosticsWithColorAndContext(
      [diagnostic],
      host,
    );
  }
  if (typeof formatted !== "string") {
    throw new M9ObservationError(
      "TypeScript formatter did not return a string",
    );
  }
  // ANSI is outside the color-free batch contract. No newline or other
  // textual normalization is allowed after this point.
  return stripAnsi(formatted);
}

export function buildRendererObservation(occurrences, cwd) {
  const firstIndexByIdentity = new Map();
  const diagnosticObjects = [];
  for (const [index, occurrence] of occurrences.entries()) {
    const diagnostic = occurrence.diagnosticObject;
    if (
      diagnostic === null ||
      typeof diagnostic !== "object" ||
      Array.isArray(diagnostic)
    ) {
      throw new M9ObservationError(
        `assembled[${index}] does not retain a diagnostic object`,
      );
    }
    if (!firstIndexByIdentity.has(diagnostic)) {
      firstIndexByIdentity.set(diagnostic, index);
    }
    diagnosticObjects.push(diagnostic);
  }

  const dedupedObjects =
    ts.sortAndDeduplicateDiagnostics(diagnosticObjects);
  const dedupedIndices = dedupedObjects.map((diagnostic, index) => {
    const assembledIndex = firstIndexByIdentity.get(diagnostic);
    if (assembledIndex === undefined) {
      throw new M9ObservationError(
        `deduped[${index}] is not an assembled diagnostic identity`,
      );
    }
    return assembledIndex;
  });
  const segments = dedupedObjects.map((diagnostic, index) => ({
    assembled_index: dedupedIndices[index],
    raw_text: formatRawDiagnostic(diagnostic, cwd),
  }));
  return {
    assembled: occurrences.map((occurrence) => occurrence.assembled),
    deduped_indices: dedupedIndices,
    segments,
    aggregate_text: segments
      .map((segment) => segment.raw_text)
      .join(""),
  };
}
