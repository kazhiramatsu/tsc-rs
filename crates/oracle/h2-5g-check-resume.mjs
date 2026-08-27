// gate-tax 5 check-side helpers for h2-5g-qualification.mjs: the
// schema-2 receipt key terms (A) and the per-case resume journal (B).
//
// Trust model: everything here is machine-local convenience state under
// target/ (gate-tax 3 precedent). A journal-adopted record can never
// corrupt the trusted state even under a buggy reader — adoption still
// passes the unchanged per-case guards (storedCaseReusable) and the
// whole-artifact byte comparison — so this library's bytes are
// deliberately NOT a receipt key term. The normalizer that COMPUTES the
// key (pin-normalize.mjs) is a key term (normalizer_sha256).
//
// Journal durability (external-review #5): one writer per file (the
// observing process appends to its own shard file), append + fsync per
// line, torn-tail-tolerant reader (the first invalid line drops the
// remainder of that file), key equality validated per line, and the
// receipt key is recomputed fresh at mint time.
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

export const JOURNAL_SCHEMA = 1;
export const JOURNAL_KIND = "h2-5g-check-resume-case";
export const RECEIPT_SCHEMA = 2;
export const RECEIPT_KIND = "h2-5g-qualification-check-receipt";

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
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

function withFingerprint(value, field) {
  return { ...value, [field]: sha256(Buffer.from(canonical(value), "utf8")) };
}

function hasValidFingerprint(value, field) {
  const payload = { ...value };
  const expected = payload[field];
  delete payload[field];
  return expected === sha256(Buffer.from(canonical(payload), "utf8"));
}

// The receipt key: every term that decides whether stored observations
// may be adopted without re-observation. `normalized_generator_sha256`
// replaces gate-tax 3's raw generator term (the incident's single
// blocking term); platform/arch cover the environment (external-review
// #6); normalizer/pin-index terms make the masking rule part of the
// producer identity (consult #2).
export function receiptKey({
  workspace,
  node,
  platform,
  arch,
  normalizerSha256,
  pinIndexRowsSha256,
  normalizedGeneratorSha256,
  globalRecordsSha256,
}) {
  const key = {
    workspace,
    node,
    platform,
    arch,
    normalizer_sha256: normalizerSha256,
    pin_index_rows_sha256: pinIndexRowsSha256,
    normalized_generator_sha256: normalizedGeneratorSha256,
    global_records_sha256: globalRecordsSha256,
  };
  return { ...key, key_sha256: sha256(Buffer.from(canonical(key), "utf8")) };
}

// Validates every receipt key term except the observation-content roll
// (the caller owns the stored-artifact parse). Returns the first
// divergent term name, or null when the receipt matches the key.
export function receiptMissTerm(receipt, key) {
  if (
    receipt === null ||
    typeof receipt !== "object" ||
    receipt.schema !== RECEIPT_SCHEMA ||
    receipt.kind !== RECEIPT_KIND ||
    !hasValidFingerprint(receipt, "receipt_fingerprint_sha256")
  ) {
    return "invalid";
  }
  if (receipt.workspace !== key.workspace) return "workspace";
  if (receipt.node !== key.node) return "node";
  if (receipt.platform !== key.platform) return "platform";
  if (receipt.arch !== key.arch) return "arch";
  if (receipt.normalizer_sha256 !== key.normalizer_sha256) return "normalizer";
  if (receipt.pin_index_rows_sha256 !== key.pin_index_rows_sha256) {
    return "pin-index";
  }
  if (receipt.normalized_generator_sha256 !== key.normalized_generator_sha256) {
    return "normalized-generator";
  }
  if (receipt.global_records_sha256 !== key.global_records_sha256) {
    return "global-records";
  }
  return null;
}

export function renderReceipt(key, generatorSha256, casesObservationSha256) {
  return withFingerprint(
    {
      schema: RECEIPT_SCHEMA,
      kind: RECEIPT_KIND,
      minted_by: "full-re-observation-check",
      workspace: key.workspace,
      node: key.node,
      platform: key.platform,
      arch: key.arch,
      normalizer_sha256: key.normalizer_sha256,
      pin_index_rows_sha256: key.pin_index_rows_sha256,
      normalized_generator_sha256: key.normalized_generator_sha256,
      // informational provenance only — NOT a key term (gate-tax 5-A):
      generator_sha256: generatorSha256,
      global_records_sha256: key.global_records_sha256,
      cases_observation_sha256: casesObservationSha256,
    },
    "receipt_fingerprint_sha256",
  );
}

// Recursive removal is confined to the journal store: every rm target
// must resolve INSIDE a `target/h2-5g/check-resume` directory and key
// directories must be 16-hex basenames. A mis-derived path throws
// instead of deleting (user directive 2026-08-27: never trust an
// unchecked recursive rmSync).
const JOURNAL_BASE_SUFFIX = path.join("target", "h2-5g", "check-resume");
const KEY_DIRECTORY_PATTERN = /^[0-9a-f]{16}$/;

function assertJournalBase(baseDirectory) {
  const resolved = path.resolve(baseDirectory);
  if (!resolved.endsWith(path.sep + JOURNAL_BASE_SUFFIX)) {
    throw new Error(
      `refusing journal removal outside ${JOURNAL_BASE_SUFFIX}: ${resolved}`,
    );
  }
  return resolved;
}

function guardedRemove(baseDirectory, entry) {
  const base = assertJournalBase(baseDirectory);
  if (!KEY_DIRECTORY_PATTERN.test(entry)) {
    throw new Error(`refusing to remove non-key journal entry: ${entry}`);
  }
  fs.rmSync(path.join(base, entry), { recursive: true, force: true });
}

export function journalDirectory(baseDirectory, key) {
  return path.join(baseDirectory, key.key_sha256.slice(0, 16));
}

// Removes journal directories minted under a different key. Only
// 16-hex key directories are touched — a foreign entry in the base is
// left alone and reported. Concurrent shard children may race the same
// removals — losing the race is fine.
export function collectStaleJournals(baseDirectory, key) {
  const keep = key === null ? null : key.key_sha256.slice(0, 16);
  let entries;
  try {
    entries = fs.readdirSync(baseDirectory);
  } catch {
    return [];
  }
  const removed = [];
  for (const entry of entries) {
    if (entry === keep || !KEY_DIRECTORY_PATTERN.test(entry)) continue;
    try {
      guardedRemove(baseDirectory, entry);
      removed.push(entry);
    } catch {
      // concurrent shard children race the same removals; losing the
      // race is fine — the stale directory is gone either way
    }
  }
  return removed;
}

export function removeJournals(baseDirectory, key) {
  guardedRemove(baseDirectory, key.key_sha256.slice(0, 16));
}

// Reads every journal file under the key's directory into a
// case_id → record map. A line is adopted only when it parses, carries a
// valid line fingerprint, and its embedded key equals the current key;
// the first invalid line in a file drops that file's remainder (torn
// tail). Returns { cases, files, droppedTails }.
export function readJournal(baseDirectory, key) {
  const directory = journalDirectory(baseDirectory, key);
  let names;
  try {
    names = fs.readdirSync(directory).filter((name) => name.endsWith(".jsonl"));
  } catch {
    return { cases: new Map(), files: 0, droppedTails: 0 };
  }
  const cases = new Map();
  let droppedTails = 0;
  for (const name of names.sort()) {
    const bytes = fs.readFileSync(path.join(directory, name), "utf8");
    for (const line of bytes.split("\n")) {
      if (line === "") continue;
      let parsed;
      try {
        parsed = JSON.parse(line);
      } catch {
        droppedTails += 1;
        break;
      }
      if (
        parsed === null ||
        typeof parsed !== "object" ||
        parsed.schema !== JOURNAL_SCHEMA ||
        parsed.kind !== JOURNAL_KIND ||
        !hasValidFingerprint(parsed, "line_fingerprint_sha256") ||
        canonical(parsed.key) !== canonical(key) ||
        typeof parsed.case_id !== "string" ||
        parsed.record === null ||
        typeof parsed.record !== "object"
      ) {
        droppedTails += 1;
        break;
      }
      if (!cases.has(parsed.case_id)) cases.set(parsed.case_id, parsed.record);
    }
  }
  return { cases, files: names.length, droppedTails };
}

// A single-writer append handle for one journal file. fsync per line: a
// kill loses at most the in-flight case.
export function openJournalWriter(baseDirectory, key, fileName) {
  const directory = journalDirectory(baseDirectory, key);
  fs.mkdirSync(directory, { recursive: true });
  const descriptor = fs.openSync(
    path.join(directory, fileName),
    fs.constants.O_WRONLY | fs.constants.O_CREAT | fs.constants.O_APPEND,
  );
  return {
    append(caseId, record) {
      const line = withFingerprint(
        {
          schema: JOURNAL_SCHEMA,
          kind: JOURNAL_KIND,
          key,
          case_id: caseId,
          record,
        },
        "line_fingerprint_sha256",
      );
      fs.writeSync(descriptor, `${JSON.stringify(line)}\n`);
      fs.fsyncSync(descriptor);
    },
    close() {
      fs.closeSync(descriptor);
    },
  };
}
