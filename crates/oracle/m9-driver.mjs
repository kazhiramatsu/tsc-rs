import readline from "node:readline";
import { pathToFileURL } from "node:url";
import {
  M9RequestError,
  assertCanonicalU64,
  assertClosedObject,
  assertLowerSha256,
  createProgramFromInline,
  decodeInlineProgram,
} from "./m9-program-host.mjs";
import {
  M9ObservationError,
  buildRendererObservation,
  collectPassOccurrences,
} from "./m9-observation.mjs";

export const M9_WIRE_SCHEMA = 1;
export const M9_PHASES = ["parse", "bind", "check", "format"];

function bindingFromRequest(request) {
  if (
    request === null ||
    typeof request !== "object" ||
    Array.isArray(request)
  ) {
    throw new M9RequestError("request must be an object");
  }
  return {
    id: assertCanonicalU64(request.id, "request.id"),
    caseSha256: assertLowerSha256(
      request.case_sha256,
      "request.case_sha256",
    ),
  };
}

export function validateExecuteCaseRequest(request) {
  const binding = bindingFromRequest(request);
  assertClosedObject(
    request,
    ["schema", "id", "op", "case_sha256", "program"],
    "request",
  );
  if (request.schema !== M9_WIRE_SCHEMA) {
    throw new M9RequestError(
      `unsupported request schema ${JSON.stringify(request.schema)}`,
    );
  }
  if (request.op !== "execute-case") {
    throw new M9RequestError(
      `unsupported request op ${JSON.stringify(request.op)}`,
    );
  }
  return {
    ...binding,
    program: decodeInlineProgram(request.program),
  };
}

function frame(binding, kind, payload) {
  return {
    schema: M9_WIRE_SCHEMA,
    id: binding.id,
    case_sha256: binding.caseSha256,
    frame: kind,
    ...payload,
  };
}

function phaseFrame(binding, phase) {
  return frame(binding, "phase", { phase });
}

function resultFrame(binding, result) {
  return frame(binding, "result", { result });
}

function errorDetail(error) {
  const detail = String(error?.stack ?? error);
  return detail.length === 0 ? "unknown Node oracle failure" : detail;
}

function terminalBoundary(phase) {
  if (phase === "parse") return "parser-invariant";
  if (phase === "format") return "renderer-invariant";
  return "phase-invariant";
}

export function terminalResult(phase, error) {
  return {
    status: "terminal",
    phase,
    kind: "panic",
    boundary_id: terminalBoundary(phase),
    detail: errorDetail(error),
  };
}

export function rejectedResult(kind, error) {
  return {
    status: "rejected",
    kind,
    detail: errorDetail(error),
  };
}

/**
 * Execute one already parsed JSON request.
 *
 * `emit` is awaited before entering each bound phase and before returning a
 * result, so the controller observes only flushed boundaries. The function
 * has no timeout, process retry, or filesystem fallback; those are
 * controller/session responsibilities.
 */
export async function executeCaseRequest(request, emit) {
  const binding = bindingFromRequest(request);
  let validated;
  try {
    validated = validateExecuteCaseRequest(request);
  } catch (error) {
    if (!(error instanceof M9RequestError)) throw error;
    const rejected = resultFrame(
      binding,
      rejectedResult("malformed-request", error),
    );
    await emit(rejected);
    return rejected;
  }

  await emit(phaseFrame(binding, "parse"));
  let programState;
  try {
    programState = createProgramFromInline(validated.program);
  } catch (error) {
    const terminal = resultFrame(
      binding,
      terminalResult("parse", error),
    );
    await emit(terminal);
    return terminal;
  }

  await emit(phaseFrame(binding, "bind"));
  try {
    // This is the explicit tsc binding boundary: createTypeChecker calls
    // initializeTypeChecker, which binds every SourceFile.
    programState.program.getTypeChecker();
  } catch (error) {
    const terminal = resultFrame(
      binding,
      terminalResult("bind", error),
    );
    await emit(terminal);
    return terminal;
  }

  await emit(phaseFrame(binding, "check"));
  let occurrences;
  try {
    // Preserve each public getter exactly. In the vendored tsc,
    // syntactic/semantic are already per-file sort/deduped while suggestion
    // is returned without an extra sort.
    occurrences = collectPassOccurrences(programState);
  } catch (error) {
    const result =
      error instanceof M9ObservationError
        ? rejectedResult("malformed-observation", error)
        : terminalResult("check", error);
    const completed = resultFrame(binding, result);
    await emit(completed);
    return completed;
  }

  await emit(phaseFrame(binding, "format"));
  let observation;
  try {
    observation = buildRendererObservation(
      occurrences,
      programState.cwd,
    );
  } catch (error) {
    const result =
      error instanceof M9ObservationError
        ? rejectedResult("malformed-observation", error)
        : terminalResult("format", error);
    const completed = resultFrame(binding, result);
    await emit(completed);
    return completed;
  }

  const completed = resultFrame(binding, {
    status: "completed",
    ...observation,
  });
  await emit(completed);
  return completed;
}

function writeFrame(output, value) {
  return new Promise((resolve, reject) => {
    output.write(`${JSON.stringify(value)}\n`, (error) => {
      if (error) reject(error);
      else resolve();
    });
  });
}

export async function serveJsonLines({
  input = process.stdin,
  output = process.stdout,
  errorOutput = process.stderr,
} = {}) {
  await writeFrame(output, {
    schema: M9_WIRE_SCHEMA,
    frame: "hello",
    implementation: "oracle-node",
    version: process.version,
  });
  const lines = readline.createInterface({
    input,
    crlfDelay: Infinity,
  });
  for await (const line of lines) {
    if (line.length === 0) {
      errorOutput.write("m9 oracle request line must not be empty\n");
      return false;
    }
    let request;
    try {
      request = JSON.parse(line);
    } catch (error) {
      errorOutput.write(
        `m9 oracle request is not JSON: ${errorDetail(error)}\n`,
      );
      return false;
    }
    try {
      if (JSON.stringify(request) !== line) {
        const binding = bindingFromRequest(request);
        await writeFrame(
          output,
          resultFrame(
            binding,
            rejectedResult(
              "malformed-request",
              new M9RequestError(
                "request must use canonical compact JSON bytes",
              ),
            ),
          ),
        );
        continue;
      }
      await executeCaseRequest(request, (value) =>
        writeFrame(output, value),
      );
    } catch (error) {
      // An invalid/missing id or case hash cannot produce a bound response.
      // Exit the session so the Rust controller records a protocol failure.
      errorOutput.write(
        `m9 oracle request cannot be bound: ${errorDetail(error)}\n`,
      );
      return false;
    }
  }
  return true;
}

const isMain =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(process.argv[1]).href;

if (isMain) {
  const success = await serveJsonLines();
  if (!success) process.exitCode = 2;
}
