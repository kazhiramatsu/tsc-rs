// Pin-literal normalizer (gate-tax 5-A): the mjs mirror of
// scripts/pin-grammar.py. Masks MANIFEST-ENUMERATED artifact-hash pin
// literals (positive selection against .github/ci/pin-index.v1.json)
// with a fixed placeholder so a receipt key can hash script bytes
// invariantly under pin repins. A grammar hit that is not enumerated —
// or an enumerated site whose current count differs — is a REFUSAL,
// never a silent mask. The five grammars are EXACTLY the
// chain-walk-repin.py patterns; the cross-check test in
// .github/ci/gate-tax-5.test.mjs asserts python/mjs agreement on every
// chain script. Pure function of (script bytes, classification rows):
// no disk reads inside the normalization itself.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const PLACEHOLDER = "<extracted-pin>";
export const PIN_INDEX_RELATIVE_PATH = ".github/ci/pin-index.v1.json";

const PAT_A =
  /"((?:ratchets|vendor|crates\/oracle|\.github)\/[^"\n]+)",\s*"([0-9a-f]{64})"/g;
const PAT_B =
  /"((?:ratchets|vendor|crates\/oracle|\.github)\/[^"\n]+)":\s*\n(\s*)"([0-9a-f]{64})"/g;
const PAT_C =
  /"((?:ratchets|vendor|crates\/oracle|\.github)\/[^"\n]+)": "([0-9a-f]{64})"/g;
const PAT_D_PAIRS = /const (\w+?)_RELATIVE_PATH =\s*\n?\s*"([^"]+)"/g;
const PAT_E_CONSTS =
  /const (\w+) =\s*\n?\s*"((?:ratchets|vendor|crates|\.github)\/[^"\n]+)"/g;

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function dHashPattern(constName) {
  return new RegExp(
    "(const " + escapeRegExp(constName) + ' =\\s*\\n?\\s*")([0-9a-f]{64})(")',
  );
}

function eHashPattern(name) {
  return new RegExp(
    "(\\[" + escapeRegExp(name) + '\\]:\\s*\\n?\\s*")([0-9a-f]{64})(")',
    "g",
  );
}

export function extractPins(text) {
  const pins = [];
  const add = (pinPath, grammar, start, end) => {
    if (!pins.some((pin) => pin.start === start && pin.end === end)) {
      pins.push({ path: pinPath, grammar, start, end });
    }
  };
  for (const match of text.matchAll(PAT_A)) {
    const end = match.index + match[0].length - 1;
    add(match[1], "A", end - 64, end);
  }
  for (const match of text.matchAll(PAT_B)) {
    const end = match.index + match[0].length - 1;
    add(match[1], "B", end - 64, end);
  }
  for (const match of text.matchAll(PAT_C)) {
    const end = match.index + match[0].length - 1;
    add(match[1], "C", end - 64, end);
  }
  const dPairs = new Map();
  for (const match of text.matchAll(PAT_D_PAIRS)) dPairs.set(match[1], match[2]);
  for (const [base, pinPath] of dPairs) {
    for (const constName of [`${base}_SHA256`, `EXPECTED_${base}_SHA256`]) {
      const match = dHashPattern(constName).exec(text);
      if (match) {
        const start = match.index + match[1].length;
        add(pinPath, "D", start, start + 64);
      }
    }
  }
  const eConsts = new Map();
  for (const match of text.matchAll(PAT_E_CONSTS)) eConsts.set(match[1], match[2]);
  for (const [name, pinPath] of eConsts) {
    for (const match of text.matchAll(eHashPattern(name))) {
      const start = match.index + match[1].length;
      add(pinPath, "E", start, start + 64);
    }
  }
  pins.sort((left, right) => left.start - right.start || left.end - right.end);
  for (let i = 1; i < pins.length; i += 1) {
    if (pins[i - 1].end > pins[i].start) {
      throw new Error(
        `overlapping extracted pin spans at ${pins[i - 1].start} and ${pins[i].start}`,
      );
    }
  }
  return pins;
}

export function classifyPins(text, rows) {
  const pins = extractPins(text);
  const pairKey = (pinPath, grammar) => JSON.stringify([pinPath, grammar]);
  const enumerated = new Map(
    (rows?.pins ?? []).map((row) => [pairKey(row.path, row.grammar), row.count]),
  );
  const semantic = new Set(
    (rows?.semantic ?? []).map((row) => pairKey(row.path, row.grammar)),
  );
  const groups = new Map();
  for (const pin of pins) {
    const key = pairKey(pin.path, pin.grammar);
    groups.set(key, (groups.get(key) ?? 0) + 1);
  }
  const masked = [];
  const refusals = [];
  for (const [key, count] of [...groups].sort()) {
    const [pinPath, grammar] = JSON.parse(key);
    if (semantic.has(key)) {
      if (enumerated.has(key)) {
        refusals.push({ path: pinPath, grammar, reason: "both-pin-and-semantic" });
      }
      continue;
    }
    if (!enumerated.has(key)) {
      refusals.push({ path: pinPath, grammar, reason: "unclassified" });
      continue;
    }
    if (enumerated.get(key) !== count) {
      refusals.push({
        path: pinPath,
        grammar,
        reason: `count-drift (${enumerated.get(key)} classified, ${count} present)`,
      });
      continue;
    }
    masked.push(
      ...pins.filter((pin) => pin.path === pinPath && pin.grammar === grammar),
    );
  }
  for (const row of rows?.pins ?? []) {
    if (!groups.has(pairKey(row.path, row.grammar))) {
      refusals.push({
        path: row.path,
        grammar: row.grammar,
        reason: "classified-but-absent",
      });
    }
  }
  masked.sort((left, right) => left.start - right.start);
  return { masked, refusals };
}

export function normalizeText(text, rows) {
  const { masked, refusals } = classifyPins(text, rows);
  if (refusals.length !== 0) return { normalized: null, refusals };
  const out = [];
  let cursor = 0;
  for (const pin of masked) {
    out.push(text.slice(cursor, pin.start), PLACEHOLDER);
    cursor = pin.end;
  }
  out.push(text.slice(cursor));
  return { normalized: out.join(""), refusals: [] };
}

export function normalizedSha256(text, rows) {
  const { normalized, refusals } = normalizeText(text, rows);
  if (refusals.length !== 0) return { sha256: null, refusals };
  return {
    sha256: crypto
      .createHash("sha256")
      .update(Buffer.from(normalized, "utf8"))
      .digest("hex"),
    refusals: [],
  };
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

export function consumerRows(index, consumer) {
  return index?.consumers?.[consumer] ?? { pins: [], semantic: [], unmatched: [] };
}

// The receipt-key term for the consumer's classification content: a
// masking-relevant index edit re-keys the receipt explicitly (the
// normalizer/pin-index version is part of the producer identity —
// external-review consult #2).
export function pinIndexRowsSha256(rows) {
  return crypto
    .createHash("sha256")
    .update(
      Buffer.from(
        canonical({ pins: rows?.pins ?? [], semantic: rows?.semantic ?? [] }),
        "utf8",
      ),
    )
    .digest("hex");
}

const invokedDirectly =
  process.argv[1] !== undefined &&
  path.resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (invokedDirectly) {
  const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const args = process.argv.slice(2);
  const script = args.shift();
  let indexPath = path.join(workspace, PIN_INDEX_RELATIVE_PATH);
  let consumer = script === undefined ? undefined : path.relative(workspace, path.resolve(script));
  while (args.length !== 0) {
    const flag = args.shift();
    if (flag === "--index") indexPath = args.shift();
    else if (flag === "--consumer") consumer = args.shift();
    else {
      process.stderr.write(`unknown flag ${flag}\n`);
      process.exit(2);
    }
  }
  if (script === undefined) {
    process.stderr.write(
      "usage: pin-normalize.mjs <script> [--index <pin-index>] [--consumer <relpath>]\n",
    );
    process.exit(2);
  }
  const text = fs.readFileSync(script, "utf8");
  const index = JSON.parse(fs.readFileSync(indexPath, "utf8"));
  const rows = consumerRows(index, consumer);
  const { sha256, refusals } = normalizedSha256(text, rows);
  if (refusals.length !== 0) {
    process.stdout.write(`${JSON.stringify({ consumer, refusals }, null, 2)}\n`);
    process.exit(3);
  }
  process.stdout.write(`${sha256}\n`);
}
