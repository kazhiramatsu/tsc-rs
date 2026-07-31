#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

const fixturePath =
  process.argv[2] ?? new URL("./canonical-class.schema1.json", import.meta.url);
const fixture = JSON.parse(readFileSync(fixturePath, "utf8"));

function fail(message) {
  throw new Error(message);
}

function plainObject(value, where) {
  if (
    value === null ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.getPrototypeOf(value) !== Object.prototype
  ) {
    fail(`${where} must be an object`);
  }
}

function exactKeys(value, expected, where) {
  plainObject(value, where);
  const actual = Object.keys(value).sort();
  const wanted = [...expected].sort();
  if (
    actual.length !== wanted.length ||
    actual.some((key, index) => key !== wanted[index])
  ) {
    fail(`${where} keys must be exactly ${wanted.join(",")}`);
  }
}

function oneOf(value, values, where) {
  if (!values.includes(value)) {
    fail(`${where} must be one of ${values.join(",")}`);
  }
}

function validUnicodeString(value, where, { nonempty = false } = {}) {
  if (typeof value !== "string" || (nonempty && value.length === 0)) {
    fail(`${where} must be ${nonempty ? "a non-empty" : "a"} string`);
  }
  for (let index = 0; index < value.length; index += 1) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff)) {
        fail(`${where} contains an unpaired high surrogate`);
      }
      index += 1;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      fail(`${where} contains an unpaired low surrogate`);
    }
  }
}

function validateFailure(failure, where) {
  plainObject(failure, where);
  if (failure.kind === "tier") {
    exactKeys(failure, ["kind", "tier"], where);
    oneOf(failure.tier, ["t0", "t1", "t2", "t3", "t4"], `${where}.tier`);
  } else if (failure.kind === "terminal") {
    exactKeys(failure, ["kind", "phase"], where);
    oneOf(
      failure.phase,
      ["parse", "bind", "check", "format"],
      `${where}.phase`,
    );
  } else {
    fail(`${where}.kind must be tier or terminal`);
  }
}

function validateRow(row, where) {
  exactKeys(row, ["side", "code", "normalized_message_head"], where);
  oneOf(row.side, ["oracle", "tsrs"], `${where}.side`);
  if (!Number.isInteger(row.code) || row.code < 0 || row.code > 0xffffffff) {
    fail(`${where}.code must be a u32`);
  }
  validUnicodeString(row.normalized_message_head, `${where}.normalized_message_head`, {
    nonempty: true,
  });
  validateNormalizedText(
    row.normalized_message_head,
    `${where}.normalized_message_head`,
  );
}

function validateRenderer(renderer, where) {
  exactKeys(renderer, ["class", "affected_key"], where);
  oneOf(renderer.class, ["order", "dedupe", "path", "newline", "text"], `${where}.class`);
  exactKeys(
    renderer.affected_key,
    ["code", "normalized_message_head"],
    `${where}.affected_key`,
  );
  if (
    !Number.isInteger(renderer.affected_key.code) ||
    renderer.affected_key.code < 0 ||
    renderer.affected_key.code > 0xffffffff
  ) {
    fail(`${where}.affected_key.code must be a u32`);
  }
  validUnicodeString(
    renderer.affected_key.normalized_message_head,
    `${where}.affected_key.normalized_message_head`,
    { nonempty: true },
  );
  validateNormalizedText(
    renderer.affected_key.normalized_message_head,
    `${where}.affected_key.normalized_message_head`,
  );
}

function rowSide(rows) {
  const oracle = rows.some((row) => row.side === "oracle");
  const tsrs = rows.some((row) => row.side === "tsrs");
  return oracle && tsrs ? "both" : oracle ? "oracle" : tsrs ? "tsrs" : "both";
}

function u32(value, where) {
  if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) {
    fail(`${where} must be a u32`);
  }
}

function canonicalDecimal(value) {
  return /^(0|[1-9][0-9]*)$/.test(value) && Number(value) <= 0xffffffff;
}

function canonicalPlaceholderAt(value, offset) {
  const rest = value.slice(offset);
  if (rest.startsWith("<@0@>")) return "<@0@>";
  const path = /^<@([12]):(0|[1-9][0-9]*)@>/.exec(rest);
  if (path !== null && canonicalDecimal(path[2])) return path[0];
  const identifier = /^<#(0|[1-9][0-9]*)#>/.exec(rest);
  if (identifier !== null && canonicalDecimal(identifier[1])) {
    return identifier[0];
  }
  return undefined;
}

function validateNormalizedText(value, where) {
  let offset = 0;
  while (offset < value.length) {
    if (value[offset] !== "<") {
      offset += String.fromCodePoint(value.codePointAt(offset)).length;
      continue;
    }
    if (value[offset + 1] === "<") {
      offset += 2;
      continue;
    }
    const placeholder = canonicalPlaceholderAt(value, offset);
    if (placeholder === undefined) {
      fail(
        `${where} contains a literal single '<' or a non-canonical placeholder`,
      );
    }
    offset += placeholder.length;
  }
}

function validateNormalization(value, where) {
  const keys = ["paths", "generated_identifiers"];
  exactKeys(value, keys, where);
  for (const key of keys) {
    if (!Array.isArray(value[key])) {
      fail(`${where}.${key} must be an array`);
    }
    const sources = new Set();
    value[key].forEach((mapping, index) => {
      const mappingWhere = `${where}.${key}[${index}]`;
      exactKeys(mapping, ["from", "to"], mappingWhere);
      validUnicodeString(mapping.from, `${mappingWhere}.from`, { nonempty: true });
      validUnicodeString(mapping.to, `${mappingWhere}.to`, { nonempty: true });
      if (key === "paths") {
        if (
          canonicalPlaceholderAt(mapping.to, 0) !== mapping.to ||
          !mapping.to.startsWith("<@")
        ) {
          fail(`${mappingWhere}.to must be a canonical schema-1 path token`);
        }
      } else if (
        canonicalPlaceholderAt(mapping.to, 0) !== mapping.to ||
        !mapping.to.startsWith("<#")
      ) {
        fail(`${mappingWhere}.to must be a canonical schema-1 generated-id token`);
      }
      if (
        key === "generated_identifiers" &&
        !/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(mapping.from)
      ) {
        fail(`${mappingWhere}.from must be an ASCII identifier`);
      }
      if (sources.has(mapping.from)) {
        fail(`${where}.${key} contains duplicate source ${JSON.stringify(mapping.from)}`);
      }
      sources.add(mapping.from);
    });
  }
  const pathSources = new Set(value.paths.map((mapping) => mapping.from));
  for (const mapping of value.generated_identifiers) {
    if (pathSources.has(mapping.from)) {
      fail(
        `${where} source ${JSON.stringify(mapping.from)} is owned by both path and generated-id roles`,
      );
    }
  }
}

function normalizeLineEndings(value) {
  return value.replace(/\r\n?/g, "\n");
}

function isPathBoundary(character) {
  if (character === undefined) return true;
  if (character.charCodeAt(0) >= 128) return false;
  return !/[A-Za-z0-9_$.\-~]/.test(character);
}

function pathLiteralAt(value, offset, source) {
  const candidates = source.includes("/")
    ? [source, source.replaceAll("/", "\\")]
    : [source];
  return candidates.find((candidate) => {
    if (!value.startsWith(candidate, offset)) return false;
    const before = offset === 0 ? undefined : [...value.slice(0, offset)].at(-1);
    const after = [...value.slice(offset + candidate.length)][0];
    const rooted = candidate.startsWith("/") || candidate.startsWith("\\");
    return (
      isPathBoundary(before) &&
      isPathBoundary(after) &&
      (rooted || !["/", "\\"].includes(before)) &&
      (rooted || !["/", "\\"].includes(after))
    );
  });
}

function asciiIdentifierContinue(character) {
  return character !== undefined && /^[A-Za-z0-9_$]$/.test(character);
}

function ownedPathAt(value, offset, mappings) {
  let selected;
  for (const mapping of mappings) {
    const literal = pathLiteralAt(value, offset, mapping.from);
    if (
      literal !== undefined &&
      (selected === undefined || literal.length > selected.literal.length)
    ) {
      selected = { mapping, literal };
    }
  }
  return selected;
}

function generatedIdentifierAt(value, offset, mappings) {
  const matches = mappings.filter((mapping) => {
    if (!value.startsWith(mapping.from, offset)) return false;
    const before = offset === 0 ? undefined : value[offset - 1];
    const after = value[offset + mapping.from.length];
    return (
      (before === undefined ||
        (before.charCodeAt(0) < 128 && !asciiIdentifierContinue(before))) &&
      (after === undefined ||
        (after.charCodeAt(0) < 128 && !asciiIdentifierContinue(after)))
    );
  });
  if (matches.length > 1) {
    fail(`ambiguous generated identifier normalization at UTF-16 offset ${offset}`);
  }
  return matches[0];
}

function normalizeOwnedText(
  value,
  normalization,
  { identifiers, newlines },
) {
  let output = "";
  let offset = 0;
  while (offset < value.length) {
    const path = ownedPathAt(value, offset, normalization.paths);
    if (path !== undefined) {
      output += path.mapping.to;
      offset += path.literal.length;
      continue;
    }
    if (identifiers) {
      const identifier = generatedIdentifierAt(
        value,
        offset,
        normalization.generated_identifiers,
      );
      if (identifier !== undefined) {
        output += identifier.to;
        offset += identifier.from.length;
        continue;
      }
    }
    if (newlines && value[offset] === "\r") {
      output += "\n";
      offset += value[offset + 1] === "\n" ? 2 : 1;
      continue;
    }
    const codePoint = String.fromCodePoint(value.codePointAt(offset));
    output += codePoint === "<" ? "<<" : codePoint;
    offset += codePoint.length;
  }
  return output;
}

function normalizeText(value, normalization) {
  return normalizeOwnedText(value, normalization, {
    identifiers: true,
    newlines: true,
  });
}

function normalizeRendererPaths(value, normalization) {
  return normalizeOwnedText(value, normalization, {
    identifiers: false,
    newlines: false,
  });
}

function validateRawDiagnostic(value, where) {
  exactKeys(
    value,
    [
      "engine",
      "pass",
      "file",
      "code",
      "line",
      "col",
      "category",
      "start",
      "length",
      "head",
      "chain",
      "related",
    ],
    where,
  );
  oneOf(value.engine, ["oracle", "tsrs"], `${where}.engine`);
  oneOf(value.pass, ["syntactic", "semantic", "suggestion"], `${where}.pass`);
  validUnicodeString(value.file, `${where}.file`, { nonempty: true });
  u32(value.code, `${where}.code`);
  for (const key of ["line", "col", "start", "length"]) {
    u32(value[key], `${where}.${key}`);
  }
  if (value.start + value.length > 0xffffffff) {
    fail(`${where}.start + length must fit u32`);
  }
  oneOf(
    value.category,
    ["warning", "error", "suggestion", "message"],
    `${where}.category`,
  );
  validUnicodeString(value.head, `${where}.head`, { nonempty: true });
  validUnicodeString(value.chain, `${where}.chain`, { nonempty: true });
  if (!Array.isArray(value.related)) {
    fail(`${where}.related must be an array`);
  }
  value.related.forEach((item, index) =>
    validUnicodeString(item, `${where}.related[${index}]`, { nonempty: true }),
  );
}

function diagnosticProjection(row, tier) {
  const prefix = [row.file, row.code, row.line, row.col];
  if (tier === "t0") return prefix;
  prefix.push(row.category);
  if (tier === "t1") return prefix;
  prefix.push(row.start, row.length);
  if (tier === "t2") return [...prefix, row.head];
  return [...prefix, row.head, row.chain, row.related];
}

function utf8Compare(left, right) {
  return Buffer.compare(Buffer.from(left, "utf8"), Buffer.from(right, "utf8"));
}

function positionFreeCompare(left, right) {
  return left.code - right.code || utf8Compare(left.head, right.head);
}

function residualRows(oracleRows, tsrsRows, normalization) {
  const collect = (rows) => {
    const grouped = new Map();
    for (const row of rows) {
      const normalized = normalizeText(row.head, normalization);
      const key = JSON.stringify([row.code, normalized]);
      const entry = grouped.get(key) ?? {
        code: row.code,
        head: normalized,
        rows: [],
      };
      entry.rows.push(row);
      grouped.set(key, entry);
    }
    return grouped;
  };
  const oracle = collect(oracleRows);
  const tsrs = collect(tsrsRows);
  const keys = new Set([...oracle.keys(), ...tsrs.keys()]);
  const oracleResidual = [];
  const tsrsResidual = [];
  const ordered = [...keys]
    .map((key) => oracle.get(key) ?? tsrs.get(key))
    .sort(positionFreeCompare);
  for (const entry of ordered) {
    const key = JSON.stringify([entry.code, entry.head]);
    const oracleEntry = oracle.get(key)?.rows ?? [];
    const tsrsEntry = tsrs.get(key)?.rows ?? [];
    if (oracleEntry.length > tsrsEntry.length) {
      oracleResidual.push(
        ...oracleEntry.slice(0, oracleEntry.length - tsrsEntry.length),
      );
    } else if (tsrsEntry.length > oracleEntry.length) {
      tsrsResidual.push(
        ...tsrsEntry.slice(0, tsrsEntry.length - oracleEntry.length),
      );
    }
  }
  const paired = Math.min(oracleResidual.length, tsrsResidual.length);
  return [oracleResidual.slice(paired), tsrsResidual.slice(paired)];
}

function diagnosticDifference(raw, tier, pass) {
  const grouped = (engine) => {
    const groups = new Map();
    for (const row of raw.diagnostics.filter(
      (candidate) => candidate.engine === engine && candidate.pass === pass,
    )) {
      const key = JSON.stringify(diagnosticProjection(row, tier));
      const rows = groups.get(key) ?? [];
      rows.push(row);
      groups.set(key, rows);
    }
    return groups;
  };
  const oracle = grouped("oracle");
  const tsrs = grouped("tsrs");
  const keys = [...new Set([...oracle.keys(), ...tsrs.keys()])].sort(utf8Compare);
  const difference = [];
  for (const key of keys) {
    const oracleRows = oracle.get(key) ?? [];
    const tsrsRows = tsrs.get(key) ?? [];
    const oracleCount = tier === "t0" ? Number(oracleRows.length !== 0) : oracleRows.length;
    const tsrsCount = tier === "t0" ? Number(tsrsRows.length !== 0) : tsrsRows.length;
    if (oracleCount === tsrsCount) continue;
    if (tier === "t0") {
      const side = oracleCount > tsrsCount ? "oracle" : "tsrs";
      const rows = side === "oracle" ? oracleRows : tsrsRows;
      difference.push(...rows.map((row) => ({ side, row })));
      continue;
    }
    const [oracleResidual, tsrsResidual] = residualRows(
      oracleRows,
      tsrsRows,
      raw.normalization,
    );
    if (oracleCount > tsrsCount) {
      difference.push(
        ...oracleResidual
          .slice(0, oracleCount - tsrsCount)
          .map((row) => ({ side: "oracle", row })),
      );
    } else {
      difference.push(
        ...tsrsResidual
          .slice(0, tsrsCount - oracleCount)
          .map((row) => ({ side: "tsrs", row })),
      );
    }
  }
  return difference;
}

function classifyDiagnostics(raw) {
  exactKeys(raw, ["kind", "normalization", "diagnostics"], "raw diagnostic");
  validateNormalization(raw.normalization, "raw diagnostic.normalization");
  if (!Array.isArray(raw.diagnostics)) {
    fail("raw diagnostic.diagnostics must be an array");
  }
  raw.diagnostics.forEach((row, index) =>
    validateRawDiagnostic(row, `raw diagnostic.diagnostics[${index}]`),
  );
  for (const tier of ["t0", "t1", "t2", "t3"]) {
    for (const pass of ["syntactic", "semantic", "suggestion"]) {
      const difference = diagnosticDifference(raw, tier, pass);
      if (difference.length === 0) continue;
      const rows = difference
        .map(({ side, row }) => ({
          side,
          code: row.code,
          normalized_message_head: normalizeText(
            row.head,
            raw.normalization,
          ),
        }))
        .sort(compareRows);
      return {
        schema: 1,
        failure: { kind: "tier", tier },
        pass,
        outcome: { side: rowSide(rows), kind: "diagnostic" },
        rows,
        renderer: null,
      };
    }
  }
  fail("raw diagnostic input has no divergence");
}

function validateRendererDiagnostic(value, where) {
  exactKeys(
    value,
    [
      "id",
      "file",
      "resolved_file",
      "start",
      "length",
      "code",
      "top_text",
      "canonical_head",
      "pass",
      "category",
      "tail",
      "related",
      "flags",
    ],
    where,
  );
  validUnicodeString(value.id, `${where}.id`, { nonempty: true });
  validUnicodeString(value.file, `${where}.file`, { nonempty: true });
  validUnicodeString(value.resolved_file, `${where}.resolved_file`, {
    nonempty: true,
  });
  u32(value.start, `${where}.start`);
  u32(value.length, `${where}.length`);
  if (value.start + value.length > 0xffffffff) {
    fail(`${where}.start + length must fit u32`);
  }
  u32(value.code, `${where}.code`);
  validUnicodeString(value.top_text, `${where}.top_text`, { nonempty: true });
  if (value.canonical_head !== null) {
    exactKeys(value.canonical_head, ["code", "message_text"], `${where}.canonical_head`);
    u32(value.canonical_head.code, `${where}.canonical_head.code`);
    if (value.canonical_head.code === 0) {
      fail(`${where}.canonical_head.code must be non-zero`);
    }
    validUnicodeString(
      value.canonical_head.message_text,
      `${where}.canonical_head.message_text`,
      { nonempty: true },
    );
  }
  oneOf(value.pass, ["syntactic", "semantic", "suggestion"], `${where}.pass`);
  oneOf(
    value.category,
    ["warning", "error", "suggestion", "message"],
    `${where}.category`,
  );
  validUnicodeString(value.tail, `${where}.tail`);
  if (!Array.isArray(value.related)) fail(`${where}.related must be an array`);
  value.related.forEach((item, index) =>
    validUnicodeString(item, `${where}.related[${index}]`, { nonempty: true }),
  );
  if (!Array.isArray(value.flags)) fail(`${where}.flags must be an array`);
  const seenFlags = new Set();
  value.flags.forEach((item, index) => {
    oneOf(
      item,
      ["reports-unnecessary", "reports-deprecated"],
      `${where}.flags[${index}]`,
    );
    if (seenFlags.has(item)) {
      fail(`${where}.flags must not contain duplicate ${item}`);
    }
    seenFlags.add(item);
  });
}

function validateRendererObservation(value, ids, where) {
  exactKeys(value, ["assembled", "deduped", "aggregate_text", "segments"], where);
  for (const key of ["assembled", "deduped"]) {
    if (!Array.isArray(value[key])) fail(`${where}.${key} must be an array`);
    value[key].forEach((id, index) => {
      validUnicodeString(id, `${where}.${key}[${index}]`, { nonempty: true });
      if (!ids.has(id)) fail(`${where}.${key}[${index}] references unknown diagnostic`);
    });
  }
  const assembledIds = new Set(value.assembled);
  value.deduped.forEach((id, index) => {
    if (!assembledIds.has(id)) {
      fail(`${where}.deduped[${index}] must select an assembled diagnostic`);
    }
  });
  validUnicodeString(value.aggregate_text, `${where}.aggregate_text`);
  if (!Array.isArray(value.segments)) fail(`${where}.segments must be an array`);
  if (value.segments.length !== value.deduped.length) {
    fail(`${where}.segments length must equal deduped length`);
  }
  value.segments.forEach((segment, index) => {
    const segmentWhere = `${where}.segments[${index}]`;
    exactKeys(segment, ["diagnostic", "raw_text"], segmentWhere);
    if (!ids.has(segment.diagnostic)) {
      fail(`${segmentWhere}.diagnostic references unknown diagnostic`);
    }
    if (segment.diagnostic !== value.deduped[index]) {
      fail(`${segmentWhere}.diagnostic must equal ${where}.deduped[${index}]`);
    }
    validUnicodeString(segment.raw_text, `${segmentWhere}.raw_text`);
  });
  const aggregate = value.segments.map((segment) => segment.raw_text).join("");
  if (aggregate !== value.aggregate_text) {
    fail(`${where}.aggregate_text must equal exact segment concatenation`);
  }
}

function effectiveDiagnostic(diagnostic) {
  return {
    file: diagnostic.resolved_file,
    start: diagnostic.start,
    length: diagnostic.length,
    code: diagnostic.canonical_head?.code ?? diagnostic.code,
    head: diagnostic.canonical_head?.message_text ?? diagnostic.top_text,
  };
}

function sequenceIdentities(observation, stage, diagnostics) {
  return observation[stage].map((id) =>
    JSON.stringify(effectiveDiagnostic(diagnostics.get(id))),
  );
}

function sameSequence(left, right) {
  return (
    left.length === right.length &&
    left.every((value, index) => value === right[index])
  );
}

function counts(values) {
  const result = new Map();
  for (const value of values) result.set(value, (result.get(value) ?? 0) + 1);
  return result;
}

function sameMultiset(left, right) {
  const a = counts(left);
  const b = counts(right);
  return (
    a.size === b.size &&
    [...a].every(([key, count]) => b.get(key) === count)
  );
}

function firstSequenceDiagnostic(raw, diagnostics, rendererClass) {
  const stage = "deduped";
  const oracleIds = raw.oracle[stage];
  const tsrsIds = raw.tsrs[stage];
  if (rendererClass === "order") {
    const oracle = sequenceIdentities(raw.oracle, stage, diagnostics);
    const tsrs = sequenceIdentities(raw.tsrs, stage, diagnostics);
    const common = Math.min(oracle.length, tsrs.length);
    let index = 0;
    while (index < common && oracle[index] === tsrs[index]) index += 1;
    return diagnostics.get(oracleIds[index] ?? tsrsIds[index]);
  }
  const oracleCounts = counts(
    sequenceIdentities(raw.oracle, stage, diagnostics),
  );
  const tsrsCounts = counts(
    sequenceIdentities(raw.tsrs, stage, diagnostics),
  );
  const completeRawKey = (diagnostic) =>
    JSON.stringify([
      diagnostic.pass,
      diagnostic.file,
      diagnostic.code,
      diagnostic.category,
      diagnostic.start,
      diagnostic.length,
      diagnostic.top_text,
      diagnostic.tail,
      diagnostic.related,
      diagnostic.flags,
      diagnostic.canonical_head,
    ]);
  return [...oracleIds, ...tsrsIds]
    .map((id) => diagnostics.get(id))
    .filter((diagnostic) => {
      const key = JSON.stringify(effectiveDiagnostic(diagnostic));
      return oracleCounts.get(key) !== tsrsCounts.get(key);
    })
    .sort(
      (left, right) =>
        left.code - right.code ||
        utf8Compare(
          normalizeText(left.top_text, raw.normalization),
          normalizeText(right.top_text, raw.normalization),
        ) ||
        utf8Compare(completeRawKey(left), completeRawKey(right)),
    )[0];
}

function firstSegmentDiagnostic(raw, diagnostics) {
  const common = Math.min(raw.oracle.segments.length, raw.tsrs.segments.length);
  for (let index = 0; index < common; index += 1) {
    const oracle = raw.oracle.segments[index];
    const tsrs = raw.tsrs.segments[index];
    if (oracle.raw_text !== tsrs.raw_text) {
      return diagnostics.get(oracle.diagnostic);
    }
  }
  const segment =
    raw.oracle.segments[common] ?? raw.tsrs.segments[common];
  if (segment === undefined) fail("renderer difference has no affected segment");
  return diagnostics.get(segment.diagnostic);
}

function classifyRenderer(raw) {
  exactKeys(
    raw,
    ["kind", "normalization", "diagnostics", "oracle", "tsrs"],
    "raw renderer",
  );
  validateNormalization(raw.normalization, "raw renderer.normalization");
  if (!Array.isArray(raw.diagnostics) || raw.diagnostics.length === 0) {
    fail("raw renderer.diagnostics must be a non-empty array");
  }
  const diagnostics = new Map();
  raw.diagnostics.forEach((diagnostic, index) => {
    validateRendererDiagnostic(diagnostic, `raw renderer.diagnostics[${index}]`);
    if (diagnostics.has(diagnostic.id)) {
      fail(`raw renderer has duplicate diagnostic id ${JSON.stringify(diagnostic.id)}`);
    }
    diagnostics.set(diagnostic.id, diagnostic);
  });
  validateRendererObservation(raw.oracle, diagnostics, "raw renderer.oracle");
  validateRendererObservation(raw.tsrs, diagnostics, "raw renderer.tsrs");

  let rendererClass;
  let affected;
  {
    const oracle = sequenceIdentities(raw.oracle, "deduped", diagnostics);
    const tsrs = sequenceIdentities(raw.tsrs, "deduped", diagnostics);
    if (!sameSequence(oracle, tsrs)) {
      rendererClass = sameMultiset(oracle, tsrs) ? "order" : "dedupe";
      affected = firstSequenceDiagnostic(raw, diagnostics, rendererClass);
    }
  }
  if (rendererClass === undefined) {
    if (raw.oracle.aggregate_text === raw.tsrs.aggregate_text) {
      fail("raw renderer input is exact");
    }
    const oraclePaths = normalizeRendererPaths(
      raw.oracle.aggregate_text,
      raw.normalization,
    );
    const tsrsPaths = normalizeRendererPaths(
      raw.tsrs.aggregate_text,
      raw.normalization,
    );
    rendererClass =
      oraclePaths === tsrsPaths
        ? "path"
        : normalizeLineEndings(raw.oracle.aggregate_text) ===
            normalizeLineEndings(raw.tsrs.aggregate_text)
          ? "newline"
          : "text";
    affected = firstSegmentDiagnostic(raw, diagnostics);
  }
  return {
    schema: 1,
    failure: { kind: "tier", tier: "t4" },
    pass: "aggregate-render",
    outcome: { side: "both", kind: "renderer" },
    rows: [],
    renderer: {
      class: rendererClass,
      affected_key: {
        code: affected.code,
        normalized_message_head: normalizeText(
          affected.top_text,
          raw.normalization,
        ),
      },
    },
  };
}

const TERMINAL_BOUNDARIES = new Set([
  "phase-invariant",
  "parser-invariant",
  "renderer-invariant",
  "renderer-state",
  "process-signal",
  "deadline",
  "allocation-limit",
  "feature-gate",
]);

function terminalPairAllowed(kind, boundary, phase) {
  if (!TERMINAL_BOUNDARIES.has(boundary)) return false;
  if (kind === "panic") {
    return (
      boundary === "phase-invariant" ||
      (boundary === "parser-invariant" && phase === "parse") ||
      (["renderer-invariant", "renderer-state"].includes(boundary) &&
        phase === "format")
    );
  }
  return (
    (kind === "crash" && boundary === "process-signal") ||
    (kind === "timeout" && boundary === "deadline") ||
    (kind === "oom" && boundary === "allocation-limit") ||
    (kind === "unsupported" && boundary === "feature-gate")
  );
}

function terminalClassAllowed(outcomeKind, phase) {
  const separator = outcomeKind.indexOf(":");
  if (
    separator <= 0 ||
    outcomeKind.indexOf(":", separator + 1) !== -1
  ) {
    return false;
  }
  return terminalPairAllowed(
    outcomeKind.slice(0, separator),
    outcomeKind.slice(separator + 1),
    phase,
  );
}

function classifyTerminal(raw) {
  exactKeys(raw, ["kind", "oracle", "tsrs"], "raw terminal");
  exactKeys(raw.oracle, ["kind"], "raw terminal.oracle");
  if (raw.oracle.kind !== "completed") {
    fail("raw terminal oracle must be completed");
  }
  exactKeys(
    raw.tsrs,
    ["kind", "phase", "terminal_kind", "boundary_id"],
    "raw terminal.tsrs",
  );
  if (raw.tsrs.kind !== "terminal") fail("raw terminal tsrs must be terminal");
  oneOf(
    raw.tsrs.phase,
    ["parse", "bind", "check", "format"],
    "raw terminal.tsrs.phase",
  );
  oneOf(
    raw.tsrs.terminal_kind,
    ["panic", "crash", "timeout", "oom", "unsupported"],
    "raw terminal.tsrs.terminal_kind",
  );
  if (
    !terminalPairAllowed(
      raw.tsrs.terminal_kind,
      raw.tsrs.boundary_id,
      raw.tsrs.phase,
    )
  ) {
    fail("raw terminal kind/boundary/phase combination is not schema-1");
  }
  return {
    schema: 1,
    failure: { kind: "terminal", phase: raw.tsrs.phase },
    pass: "terminal",
    outcome: {
      side: "tsrs",
      kind: `${raw.tsrs.terminal_kind}:${raw.tsrs.boundary_id}`,
    },
    rows: [],
    renderer: null,
  };
}

function classifyRaw(raw) {
  plainObject(raw, "raw");
  if (raw.kind === "diagnostic") return classifyDiagnostics(raw);
  if (raw.kind === "renderer") return classifyRenderer(raw);
  if (raw.kind === "terminal") return classifyTerminal(raw);
  fail("raw.kind must be diagnostic, renderer, or terminal");
}

function validateClass(value, where) {
  exactKeys(
    value,
    ["schema", "failure", "pass", "outcome", "rows", "renderer"],
    where,
  );
  if (value.schema !== 1) {
    fail(`${where}.schema must be 1`);
  }
  validateFailure(value.failure, `${where}.failure`);
  oneOf(
    value.pass,
    ["syntactic", "semantic", "suggestion", "aggregate-render", "terminal"],
    `${where}.pass`,
  );
  exactKeys(value.outcome, ["side", "kind"], `${where}.outcome`);
  oneOf(value.outcome.side, ["oracle", "tsrs", "both"], `${where}.outcome.side`);
  validUnicodeString(value.outcome.kind, `${where}.outcome.kind`, { nonempty: true });
  if (!Array.isArray(value.rows)) {
    fail(`${where}.rows must be an array`);
  }
  value.rows.forEach((row, index) => validateRow(row, `${where}.rows[${index}]`));
  if (
    value.rows.some(
      (row, index) => index > 0 && compareRows(value.rows[index - 1], row) > 0,
    )
  ) {
    fail(`${where}.rows must already be sorted by side/code/UTF-8 head bytes`);
  }
  if (value.renderer !== null) {
    validateRenderer(value.renderer, `${where}.renderer`);
  }

  if (value.failure.kind === "tier" && value.failure.tier === "t4") {
    if (
      value.pass !== "aggregate-render" ||
      value.outcome.side !== "both" ||
      value.outcome.kind !== "renderer" ||
      value.rows.length !== 0 ||
      value.renderer === null
    ) {
      fail(`${where} has an invalid T4 shape`);
    }
  } else if (value.failure.kind === "tier") {
    if (
      !["syntactic", "semantic", "suggestion"].includes(value.pass) ||
      value.outcome.kind !== "diagnostic" ||
      value.rows.length === 0 ||
      value.renderer !== null ||
      value.outcome.side !== rowSide(value.rows)
    ) {
      fail(`${where} has an invalid T0-T3 shape`);
    }
  } else if (
    value.pass !== "terminal" ||
    value.outcome.side !== "tsrs" ||
    value.rows.length !== 0 ||
    value.renderer !== null ||
    !terminalClassAllowed(value.outcome.kind, value.failure.phase)
  ) {
    fail(`${where} has an invalid terminal shape`);
  }
}

const sideRank = new Map([
  ["oracle", 0],
  ["tsrs", 1],
]);

function compareRows(left, right) {
  const side = sideRank.get(left.side) - sideRank.get(right.side);
  if (side !== 0) return side;
  if (left.code !== right.code) return left.code - right.code;
  return Buffer.compare(
    Buffer.from(left.normalized_message_head, "utf8"),
    Buffer.from(right.normalized_message_head, "utf8"),
  );
}

function canonicalObject(value) {
  const failure =
    value.failure.kind === "tier"
      ? { kind: value.failure.kind, tier: value.failure.tier }
      : { kind: value.failure.kind, phase: value.failure.phase };
  const rows = [...value.rows].sort(compareRows).map((row) => ({
    side: row.side,
    code: row.code,
    normalized_message_head: row.normalized_message_head,
  }));
  const renderer =
    value.renderer === null
      ? null
      : {
          class: value.renderer.class,
          affected_key: {
            code: value.renderer.affected_key.code,
            normalized_message_head:
              value.renderer.affected_key.normalized_message_head,
          },
        };
  return {
    schema: value.schema,
    failure,
    pass: value.pass,
    outcome: {
      side: value.outcome.side,
      kind: value.outcome.kind,
    },
    rows,
    renderer,
  };
}

exactKeys(fixture, ["schema", "rejection_canaries", "vectors"], "fixture");
if (fixture.schema !== 1 || !Array.isArray(fixture.vectors) || fixture.vectors.length === 0) {
  fail("fixture must be non-empty schema 1");
}
exactKeys(
  fixture.rejection_canaries,
  [
    "terminal_boundary_ids",
    "normalization_cross_role_source",
    "renderer_foreign_deduped",
  ],
  "fixture.rejection_canaries",
);
if (
  !Array.isArray(fixture.rejection_canaries.terminal_boundary_ids) ||
  JSON.stringify(fixture.rejection_canaries.terminal_boundary_ids) !==
    JSON.stringify(["seed42", "timestamp", "hash"])
) {
  fail("terminal boundary rejection canary ids must remain schema-pinned");
}
for (const boundary of fixture.rejection_canaries.terminal_boundary_ids) {
  validUnicodeString(boundary, "terminal boundary rejection canary", {
    nonempty: true,
  });
  if (
    ["panic", "crash", "timeout", "oom", "unsupported"].some((kind) =>
      ["parse", "bind", "check", "format"].some((phase) =>
        terminalPairAllowed(kind, boundary, phase),
      ),
    )
  ) {
    fail(`terminal rejection canary ${JSON.stringify(boundary)} was accepted`);
  }
}
const crossRoleSource =
  fixture.rejection_canaries.normalization_cross_role_source;
validUnicodeString(crossRoleSource, "normalization cross-role rejection canary", {
  nonempty: true,
});
if (crossRoleSource !== "owned") {
  fail("normalization cross-role rejection source must remain schema-pinned");
}
let crossRoleRejected = false;
try {
  validateNormalization(
    {
      paths: [{ from: crossRoleSource, to: "<@2:0@>" }],
      generated_identifiers: [{ from: crossRoleSource, to: "<#0#>" }],
    },
    "normalization cross-role rejection canary",
  );
} catch {
  crossRoleRejected = true;
}
if (!crossRoleRejected) {
  fail("normalization cross-role ownership canary was accepted");
}
const foreignDeduped = fixture.rejection_canaries.renderer_foreign_deduped;
exactKeys(
  foreignDeduped,
  ["assembled_id", "foreign_id"],
  "renderer foreign-deduped rejection canary",
);
for (const key of ["assembled_id", "foreign_id"]) {
  validUnicodeString(
    foreignDeduped[key],
    `renderer foreign-deduped rejection canary.${key}`,
    { nonempty: true },
  );
}
if (foreignDeduped.assembled_id === foreignDeduped.foreign_id) {
  fail("renderer foreign-deduped rejection canary ids must differ");
}
let foreignDedupedRejected = false;
try {
  validateRendererObservation(
    {
      assembled: [foreignDeduped.assembled_id],
      deduped: [foreignDeduped.foreign_id],
      aggregate_text: "",
      segments: [
        {
          diagnostic: foreignDeduped.foreign_id,
          raw_text: "",
        },
      ],
    },
    new Set([foreignDeduped.assembled_id, foreignDeduped.foreign_id]),
    "renderer foreign-deduped rejection canary",
  );
} catch {
  foreignDedupedRejected = true;
}
if (!foreignDedupedRejected) {
  fail("renderer foreign-deduped canary was accepted");
}

const ids = new Set();
const vectorsById = new Map();
for (const [index, vector] of fixture.vectors.entries()) {
  const where = `fixture.vectors[${index}]`;
  exactKeys(vector, ["id", "raw", "class", "canonical_utf8", "sha256"], where);
  validUnicodeString(vector.id, `${where}.id`, { nonempty: true });
  if (ids.has(vector.id)) {
    fail(`${where}.id duplicates ${JSON.stringify(vector.id)}`);
  }
  ids.add(vector.id);
  vectorsById.set(vector.id, vector);
  validateClass(vector.class, `${where}.class`);
  const derived = classifyRaw(vector.raw);
  validateClass(derived, `${where}.raw derived class`);
  const derivedCanonical = JSON.stringify(canonicalObject(derived));
  const declaredCanonical = JSON.stringify(canonicalObject(vector.class));
  if (derivedCanonical !== declaredCanonical) {
    fail(
      `${where}.class does not match independently classified raw input` +
        `\nderived: ${JSON.stringify(derivedCanonical)}` +
        `\ndeclared: ${JSON.stringify(declaredCanonical)}`,
    );
  }
  validUnicodeString(vector.canonical_utf8, `${where}.canonical_utf8`);
  if (!/^[0-9a-f]{64}$/.test(vector.sha256)) {
    fail(`${where}.sha256 must be an exact lowercase SHA-256`);
  }

  const canonical = derivedCanonical;
  if (canonical !== vector.canonical_utf8) {
    fail(`${where}.canonical_utf8 mismatch\nactual: ${JSON.stringify(canonical)}`);
  }
  const sha256 = createHash("sha256")
    .update(Buffer.from(canonical, "utf8"))
    .digest("hex");
  if (sha256 !== vector.sha256) {
    fail(`${where}.sha256 mismatch: expected ${vector.sha256}, actual ${sha256}`);
  }
}

function requiredVector(id) {
  const vector = vectorsById.get(id);
  if (vector === undefined) {
    fail(`fixture is missing required vector ${JSON.stringify(id)}`);
  }
  return vector;
}

const passMatrix = [
  ["syntactic", requiredVector("diagnostic-pass-syntactic-placeholders")],
  ["semantic", requiredVector("diagnostic-pass-semantic-placeholders")],
  ["suggestion", requiredVector("diagnostic-pass-suggestion-placeholders")],
];
const diagnosticShape = (value) =>
  JSON.stringify({
    failure: value.failure,
    outcome: value.outcome,
    rows: value.rows,
    renderer: value.renderer,
  });
const commonPassShape = diagnosticShape(passMatrix[0][1].class);
for (const [pass, vector] of passMatrix) {
  if (
    vector.class.pass !== pass ||
    diagnosticShape(vector.class) !== commonPassShape
  ) {
    fail(`diagnostic ${pass} vector must differ from the pass matrix only by pass`);
  }
}
const placeholderHead =
  passMatrix[0][1].class.rows[0]?.normalized_message_head ?? "";
if (!placeholderHead.includes("<#0#>") || !placeholderHead.includes("<@2:0@>")) {
  fail("diagnostic pass matrix must retain both schema-1 normalization placeholders");
}

const coveredTiers = new Set(
  fixture.vectors
    .filter((vector) => vector.class.failure.kind === "tier")
    .map((vector) => vector.class.failure.tier),
);
for (const tier of ["t0", "t1", "t2", "t3", "t4"]) {
  if (!coveredTiers.has(tier)) {
    fail(`fixture is missing canonical comparison tier ${tier}`);
  }
}

const rendererClasses = new Set(
  fixture.vectors
    .filter((vector) => vector.class.failure.kind === "tier")
    .map((vector) => vector.class.renderer?.class)
    .filter((value) => value !== undefined),
);
for (const rendererClass of ["order", "dedupe", "path", "newline", "text"]) {
  if (!rendererClasses.has(rendererClass)) {
    fail(`fixture is missing T4 renderer class ${rendererClass}`);
  }
}

const terminalPhases = new Map();
for (const vector of fixture.vectors.filter(
  (candidate) => candidate.class.failure.kind === "terminal",
)) {
  const kind = vector.class.outcome.kind.slice(
    0,
    vector.class.outcome.kind.indexOf(":"),
  );
  if (terminalPhases.has(kind)) {
    fail(`fixture has duplicate terminal-kind vector ${kind}`);
  }
  terminalPhases.set(kind, vector.class.failure.phase);
}
for (const [kind, phase] of [
  ["panic", "parse"],
  ["crash", "bind"],
  ["timeout", "format"],
  ["oom", "check"],
  ["unsupported", "bind"],
]) {
  if (terminalPhases.get(kind) !== phase) {
    fail(`terminal kind ${kind} must use fixed representative phase ${phase}`);
  }
}

console.log(`verified ${fixture.vectors.length} canonical class schema-1 vectors`);
