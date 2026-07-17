// A2 exact scope identity — Node half of the versioned canonical
// occurrence encoder (measurement-integrity.md §3, encoder v1).
//
// Mirrors crates/conformance/src/identity.rs byte for byte. The scope
// audit feeds golden oracle records and the committed vector file
// through both encoders and fails on any difference. This tool is
// check-only and NOT part of the golden-producing path (driver.mjs /
// program-host.mjs); it must never be imported from the pinned
// producer modules, or the A1 producer pin would go stale.
//
// stdin:  {"encoder":1,"cases":[{"fixture","matrix_key","records":[...]}]}
// stdout: {"encoder":1,"cases":[{"record_canonical":[...],
//          "identities":[...],"identity_canonical":[...],
//          "identity_sha256":[...]}]}

import { createHash } from "node:crypto";

const ENCODER_VERSION = 1;

// JSON string escaping, matching the Rust writer exactly: `"`, `\`,
// shorthands \b \t \n \f \r, other control chars below 0x20 as
// lowercase \u00xx, everything else raw UTF-8. Deliberately explicit
// instead of JSON.stringify so both languages pin the same table.
function escapeString(value) {
  let out = '"';
  for (const ch of value) {
    const point = ch.codePointAt(0);
    if (ch === '"') out += '\\"';
    else if (ch === "\\") out += "\\\\";
    else if (point === 0x08) out += "\\b";
    else if (point === 0x09) out += "\\t";
    else if (point === 0x0a) out += "\\n";
    else if (point === 0x0c) out += "\\f";
    else if (point === 0x0d) out += "\\r";
    else if (point < 0x20) out += "\\u" + point.toString(16).padStart(4, "0");
    else out += ch;
  }
  return out + '"';
}

function encodeOptString(value) {
  return value === null || value === undefined ? "null" : escapeString(value);
}

function encodeOptInt(value) {
  if (value === null || value === undefined) return "null";
  if (!Number.isInteger(value) || value < 0) {
    throw new Error(`canonical integers are unsigned decimals, got ${value}`);
  }
  return String(value);
}

function encodeChain(chain) {
  const next = (chain.next ?? []).map(encodeChain).join(",");
  return (
    `{"category":${escapeString(chain.category)}` +
    `,"code":${encodeOptInt(chain.code)}` +
    `,"next":[${next}]` +
    `,"text":${escapeString(chain.text)}}`
  );
}

function encodeRelated(related) {
  const entries = (related ?? []).map(
    (entry) =>
      `{"category":${escapeString(entry.category)}` +
      `,"chain":${encodeChain(entry.chain)}` +
      `,"code":${encodeOptInt(entry.code)}` +
      `,"file":${encodeOptString(entry.file)}` +
      `,"length":${encodeOptInt(entry.length)}` +
      `,"start":${encodeOptInt(entry.start)}}`,
  );
  return `[${entries.join(",")}]`;
}

function encodeRecord(diag) {
  return (
    `{"category":${escapeString(diag.category)}` +
    `,"chain":${encodeChain(diag.chain)}` +
    `,"code":${encodeOptInt(diag.code)}` +
    `,"col":${encodeOptInt(diag.col)}` +
    `,"file":${encodeOptString(diag.file)}` +
    `,"length":${encodeOptInt(diag.length)}` +
    `,"line":${encodeOptInt(diag.line)}` +
    `,"pass":${encodeOptString(diag.pass)}` +
    `,"related":${encodeRelated(diag.related)}` +
    `,"reports_deprecated":${diag.reports_deprecated ? "true" : "false"}` +
    `,"reports_unnecessary":${diag.reports_unnecessary ? "true" : "false"}` +
    `,"source":${encodeOptString(diag.source)}` +
    `,"start":${encodeOptInt(diag.start)}}`
  );
}

function encodeIdentity(identity) {
  return (
    `{"category":${escapeString(identity.category)}` +
    `,"chain_sha256":${escapeString(identity.chain_sha256)}` +
    `,"code":${encodeOptInt(identity.code)}` +
    `,"file":${encodeOptString(identity.file)}` +
    `,"fixture":${escapeString(identity.fixture)}` +
    `,"length":${encodeOptInt(identity.length)}` +
    `,"matrix_key":${escapeString(identity.matrix_key)}` +
    `,"occurrence":${encodeOptInt(identity.occurrence)}` +
    `,"pass":${escapeString(identity.pass)}` +
    `,"related_sha256":${escapeString(identity.related_sha256)}` +
    `,"start":${encodeOptInt(identity.start)}}`
  );
}

function sha256Hex(text) {
  return createHash("sha256").update(Buffer.from(text, "utf8")).digest("hex");
}

function caseReport(fixture, matrixKey, records) {
  const identities = records.map((diag) => {
    if (diag.pass === null || diag.pass === undefined) {
      throw new Error(
        `golden ${fixture} [${matrixKey}] lacks pass provenance for code ${diag.code}`,
      );
    }
    return {
      fixture,
      matrix_key: matrixKey,
      pass: diag.pass,
      file: diag.file ?? null,
      start: diag.start ?? null,
      length: diag.length ?? null,
      code: diag.code,
      category: diag.category,
      chain_sha256: sha256Hex(encodeChain(diag.chain)),
      related_sha256: sha256Hex(encodeRelated(diag.related)),
      occurrence: 0,
    };
  });

  // Stable sort by complete canonical record BYTES (UTF-8 order, not
  // UTF-16 code-unit order), byte-identical neighbors retaining input
  // order; then number occurrences per identity tuple in that order.
  const recordCanonical = records.map(encodeRecord);
  const recordBytes = recordCanonical.map((text) => Buffer.from(text, "utf8"));
  const order = recordBytes
    .map((_, index) => index)
    .sort((a, b) => Buffer.compare(recordBytes[a], recordBytes[b]) || a - b);
  const counts = new Map();
  for (const index of order) {
    const tupleKey = encodeIdentity(identities[index]); // occurrence still 0
    const seen = counts.get(tupleKey) ?? 0;
    identities[index].occurrence = seen;
    counts.set(tupleKey, seen + 1);
  }

  return {
    record_canonical: recordCanonical,
    identities,
    identity_canonical: identities.map(encodeIdentity),
    identity_sha256: identities.map((identity) =>
      sha256Hex(encodeIdentity(identity)),
    ),
  };
}

async function main() {
  const chunks = [];
  for await (const chunk of process.stdin) chunks.push(chunk);
  const input = JSON.parse(Buffer.concat(chunks).toString("utf8"));
  if (input.encoder !== ENCODER_VERSION) {
    throw new Error(
      `unsupported encoder version ${input.encoder} (this tool implements v${ENCODER_VERSION})`,
    );
  }
  const cases = input.cases.map((entry) =>
    caseReport(entry.fixture, entry.matrix_key, entry.records),
  );
  process.stdout.write(JSON.stringify({ encoder: ENCODER_VERSION, cases }));
}

main().catch((error) => {
  console.error(String(error?.stack ?? error));
  process.exit(1);
});
