import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const workspace = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const policyPath = path.join(workspace, ".github/ci/qualification-policy.v2.json");
const contractDirectory = path.join(workspace, ".github/ci/contracts");
const JSON_SCHEMA_MAX_DEPTH = 256;
const RUST_SOURCE_MAX_BYTES = 4 * 1024 * 1024;
const RUST_LEXICAL_NESTING_MAX = 256;

const HOSTED_ACCEPTANCE_MODULES = Object.freeze([
  "crates/xtask/src/bounded_pipeline.rs",
  "crates/xtask/src/h1_emit_acceptance.rs",
  "crates/xtask/src/h2_1a_acceptance.rs",
  "crates/xtask/src/h2_1b_acceptance.rs",
  "crates/xtask/src/h2_1c_acceptance.rs",
  "crates/xtask/src/h2_1d_acceptance.rs",
  "crates/xtask/src/h2_1e_acceptance.rs",
  "crates/xtask/src/h2_2a_acceptance.rs",
  "crates/xtask/src/h2_2b_acceptance.rs",
  "crates/xtask/src/h2_2c_acceptance.rs",
  "crates/xtask/src/h2_2d_acceptance.rs",
  "crates/xtask/src/h2_3a_acceptance.rs",
  "crates/xtask/src/h2_3b_acceptance.rs",
  "crates/xtask/src/h2_3c_acceptance.rs",
  "crates/xtask/src/h2_3d_acceptance.rs",
]);

const H2_OWNER_SPLIT_MODULES = Object.freeze([
  "crates/xtask/src/h2_3b_acceptance.rs",
  "crates/xtask/src/h2_3c_acceptance.rs",
  "crates/xtask/src/h2_3d_acceptance.rs",
]);

const H2_LOCAL_OWNER_CALLS = Object.freeze([
  "h2_3b_acceptance::run_owner_controls",
  "h2_3c_acceptance::run_owner_controls",
  "h2_3d_acceptance::run_owner_controls",
  "h2_3d_acceptance::run_h2_4a_owner_controls",
  "h2_3d_acceptance::run_h2_4b_owner_controls",
  "h2_3d_acceptance::run_h2_5a_owner_controls",
  "h2_3d_acceptance::run_h2_5b_owner_controls",
  "h2_3d_acceptance::run_h2_5c_owner_controls",
  "h2_3d_acceptance::run_h2_5d_owner_controls",
  "h2_3d_acceptance::run_h2_5e_owner_controls",
  "h2_3d_acceptance::run_h2_5f_owner_controls",
  "h2_3d_acceptance::run_h2_5g_owner_controls",
]);

const HOSTED_ACCEPTANCE_QUALIFIED_CALLS = Object.freeze([
  "std::iter::empty",
  "h1_emit_acceptance::run",
  "h2_1a_acceptance::run",
  "h2_1b_acceptance::run",
  "h2_1c_acceptance::run",
  "h2_1d_acceptance::run",
  "h2_1e_acceptance::run",
  "h2_2a_acceptance::run",
  "h2_2b_acceptance::run",
  "h2_2c_acceptance::run",
  "h2_2d_acceptance::run",
  "h2_3a_acceptance::run",
  "h2_3b_acceptance::run",
  "h2_3c_acceptance::run",
  "h2_3d_acceptance::run",
  "h2_2c_acceptance::run_h2_4a",
  "h2_2c_acceptance::run_h2_4b",
  "h2_2c_acceptance::run_h2_5a",
  "h2_2c_acceptance::run_h2_5b",
  "h2_2c_acceptance::run_h2_5c",
  "h2_2c_acceptance::run_h2_5d",
  "h2_2c_acceptance::run_h2_5e",
  "h2_2c_acceptance::run_h2_5f",
  "h2_2c_acceptance::run_h2_5g",
  "h2_2c_acceptance::run_h2_5h",
  "h2_2c_acceptance::run_h2_6a",
]);

const HOSTED_ACCEPTANCE_CANONICAL_BODY = [
  "ifletSome(argument)=args.next(){returnErr(format!().into());}",
  "conformance(std::iter::empty())?;",
  "letworkspace=find_workspace_root()?;",
  ...HOSTED_ACCEPTANCE_QUALIFIED_CALLS.slice(1).map((callee, index, calls) =>
    index + 1 === calls.length ? `${callee}(&workspace)` : `${callee}(&workspace)?;`,
  ),
].join("");

export const ARTIFACT_SCHEMA_CONTRACTS = Object.freeze([
  Object.freeze({
    label: "H2.5g qualification",
    schema: ".github/ci/contracts/h2-5g-qualification.schema.json",
    artifact: "ratchets/h2-5g-qualification.v1.json",
  }),
  Object.freeze({
    label: "H2.5g owner controls",
    schema: ".github/ci/contracts/h2-5g-owner-controls.schema.json",
    artifact: "ratchets/h2-5g-owner-controls.v1.json",
  }),
  Object.freeze({
    label: "H2.5g profile",
    schema: ".github/ci/contracts/h2-5g-profile.schema.json",
    artifact: "ratchets/h2-5g-profile.v1.json",
  }),
  Object.freeze({
    label: "H2.5h qualification",
    schema: ".github/ci/contracts/h2-5h-qualification.schema.json",
    artifact: "ratchets/h2-5h-qualification.v1.json",
  }),
  Object.freeze({
    label: "H2.6a qualification",
    schema: ".github/ci/contracts/h2-6a-qualification.schema.json",
    artifact: "ratchets/h2-6a-qualification.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a foundation",
    schema: ".github/ci/contracts/h2-5h-a-foundation.schema.json",
    artifact: "ratchets/h2-5h-a-foundation.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a comment-scope witnesses",
    schema: ".github/ci/contracts/h2-5h-a-comment-scope-witnesses.schema.json",
    artifact: "ratchets/h2-5h-a-comment-scope-witnesses.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a owner graph",
    schema: ".github/ci/contracts/h2-5h-a-owner-graph.schema.json",
    artifact: "ratchets/h2-5h-a-owner-graph.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a local-gap matrix",
    schema: ".github/ci/contracts/h2-5h-a-gap-matrix.schema.json",
    artifact: "ratchets/h2-5h-a-gap-matrix.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a architecture dispositions",
    schema: ".github/ci/contracts/h2-5h-a-dispositions.schema.json",
    artifact: "ratchets/h2-5h-a-dispositions.v1.json",
  }),
  Object.freeze({
    label: "H2.5h-a ES2015/Generators witnesses",
    schema: ".github/ci/contracts/h2-5h-a-es2015-generators-witnesses.schema.json",
    artifact: "ratchets/h2-5h-a-es2015-generators-witnesses.v1.json",
  }),
  Object.freeze({
    label: "H2.6a source-map witnesses",
    schema: ".github/ci/contracts/h2-6a-witnesses.schema.json",
    artifact: "ratchets/h2-6a-witnesses.v1.json",
  }),
]);

const JSON_SCHEMA_KEYWORDS = new Set([
  "$schema",
  "$id",
  "title",
  "$defs",
  "$ref",
  "type",
  "const",
  "enum",
  "properties",
  "required",
  "additionalProperties",
  "items",
  "minItems",
  "maxItems",
  "uniqueItems",
  "minLength",
  "maxLength",
  "pattern",
  "minimum",
  "maximum",
  "exclusiveMinimum",
  "exclusiveMaximum",
  "oneOf",
  "allOf",
  "if",
  "then",
]);

const JSON_SCHEMA_TYPES = new Set([
  "null",
  "object",
  "array",
  "string",
  "boolean",
  "number",
  "integer",
]);

export function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function exactKeys(value, required, optional = []) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const allowed = new Set([...required, ...optional]);
  return required.every((key) => Object.hasOwn(value, key)) && Object.keys(value).every((key) => allowed.has(key));
}

function isSha1(value) {
  return typeof value === "string" && /^[0-9a-f]{40}$/u.test(value);
}

function isSha256(value) {
  return typeof value === "string" && /^[0-9a-f]{64}$/u.test(value);
}

function isRelativeRepositoryPath(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 4096 &&
    !path.posix.isAbsolute(value) &&
    !value.split("/").includes("..") &&
    !value.includes("\\")
  );
}

function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(",")}]`;
  if (value && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function isJsonObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function escapeJsonPointerToken(value) {
  return value.replaceAll("~", "~0").replaceAll("/", "~1");
}

function appendJsonPointer(pointer, token) {
  return `${pointer}/${escapeJsonPointerToken(token)}`;
}

function displayJsonPointer(pointer) {
  return pointer.length === 0 ? "/" : pointer;
}

function schemaSubsetError(schemaPath, message) {
  throw new Error(`unsupported JSON schema at ${displayJsonPointer(schemaPath)}: ${message}`);
}

function findJsonValueIssue(value, valuePath = "", depth = 0, active = new Set()) {
  if (depth > JSON_SCHEMA_MAX_DEPTH) {
    return { valuePath, message: "JSON value nesting exceeds its bound" };
  }
  if (value === null || typeof value === "string" || typeof value === "boolean") return undefined;
  if (typeof value === "number") {
    return Number.isFinite(value)
      ? undefined
      : { valuePath, message: "non-finite numbers are not JSON-compatible" };
  }
  if (typeof value !== "object") {
    return { valuePath, message: `${typeof value} values are not JSON-compatible` };
  }
  if (active.has(value)) return { valuePath, message: "cyclic values are not JSON-compatible" };
  active.add(value);
  try {
    if (Array.isArray(value)) {
      for (let index = 0; index < value.length; index += 1) {
        const itemPath = appendJsonPointer(valuePath, String(index));
        if (!Object.hasOwn(value, index)) {
          return { valuePath: itemPath, message: "sparse arrays are not JSON-compatible" };
        }
        const issue = findJsonValueIssue(value[index], itemPath, depth + 1, active);
        if (issue) return issue;
      }
      return undefined;
    }
    for (const [name, child] of Object.entries(value)) {
      const issue = findJsonValueIssue(
        child,
        appendJsonPointer(valuePath, name),
        depth + 1,
        active,
      );
      if (issue) return issue;
    }
    return undefined;
  } finally {
    active.delete(value);
  }
}

function canonicalSchemaValue(value, schemaPath) {
  const issue = findJsonValueIssue(value, schemaPath);
  if (issue) schemaSubsetError(issue.valuePath, issue.message);
  try {
    const result = canonical(value);
    if (typeof result !== "string") schemaSubsetError(schemaPath, "value is not JSON-compatible");
    return result;
  } catch (error) {
    if (error instanceof Error && error.message.startsWith("unsupported JSON schema")) throw error;
    schemaSubsetError(schemaPath, `value is not JSON-compatible: ${error.message}`);
  }
}

function decodeJsonPointerToken(token, schemaPath) {
  if (/~(?:[^01]|$)/u.test(token)) schemaSubsetError(schemaPath, "local $ref contains an invalid JSON Pointer escape");
  return token.replaceAll("~1", "/").replaceAll("~0", "~");
}

function resolveLocalSchemaReference(rootSchema, reference, schemaPath) {
  if (typeof reference !== "string" || (reference !== "#" && !reference.startsWith("#/"))) {
    schemaSubsetError(schemaPath, "$ref must be a local JSON Pointer");
  }
  if (reference.includes("%")) {
    schemaSubsetError(schemaPath, "percent-encoding is not supported in local $ref");
  }
  let target = rootSchema;
  if (reference !== "#") {
    for (const encodedToken of reference.slice(2).split("/")) {
      const token = decodeJsonPointerToken(encodedToken, schemaPath);
      if ((typeof target !== "object" || target === null) || !Object.hasOwn(target, token)) {
        schemaSubsetError(schemaPath, `unresolved local $ref ${reference}`);
      }
      target = target[token];
    }
  }
  if (!isJsonObject(target)) schemaSubsetError(schemaPath, `$ref ${reference} does not resolve to a schema object`);
  return { target, schemaPath: reference };
}

function prepareJsonSchemaSubset(rootSchema) {
  if (!isJsonObject(rootSchema)) schemaSubsetError("#", "root schema must be an object");
  const references = new WeakMap();
  const patterns = new WeakMap();
  const constValues = new WeakMap();
  const enumValues = new WeakMap();
  const active = new Set();
  const complete = new Set();

  function visit(schema, schemaPath, depth) {
    if (!isJsonObject(schema)) schemaSubsetError(schemaPath, "schema must be an object");
    if (depth > JSON_SCHEMA_MAX_DEPTH) schemaSubsetError(schemaPath, "schema nesting exceeds its bound");
    if (complete.has(schema)) return;
    if (active.has(schema)) schemaSubsetError(schemaPath, "cyclic local $ref is not supported");
    active.add(schema);
    try {
      for (const keyword of Object.keys(schema)) {
        if (!JSON_SCHEMA_KEYWORDS.has(keyword)) {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `unsupported keyword ${keyword}`);
        }
      }
      for (const keyword of ["$schema", "$id", "title"]) {
        if (Object.hasOwn(schema, keyword) && typeof schema[keyword] !== "string") {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `${keyword} must be a string`);
        }
      }
      for (const keyword of ["$schema", "$id"]) {
        if (schema !== rootSchema && Object.hasOwn(schema, keyword)) {
          schemaSubsetError(
            appendJsonPointer(schemaPath, keyword),
            `${keyword} is supported only on the root schema`,
          );
        }
      }
      if (
        schema === rootSchema &&
        Object.hasOwn(schema, "$schema") &&
        schema.$schema !== "https://json-schema.org/draft/2020-12/schema"
      ) {
        schemaSubsetError(
          appendJsonPointer(schemaPath, "$schema"),
          "$schema must select JSON Schema draft 2020-12",
        );
      }
      if (Object.hasOwn(schema, "$ref")) {
        const resolved = resolveLocalSchemaReference(
          rootSchema,
          schema.$ref,
          appendJsonPointer(schemaPath, "$ref"),
        );
        references.set(schema, resolved);
        visit(resolved.target, resolved.schemaPath, depth + 1);
      }
      for (const keyword of ["$defs", "properties"]) {
        if (!Object.hasOwn(schema, keyword)) continue;
        if (!isJsonObject(schema[keyword])) {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `${keyword} must be an object`);
        }
        for (const [name, child] of Object.entries(schema[keyword])) {
          visit(child, appendJsonPointer(appendJsonPointer(schemaPath, keyword), name), depth + 1);
        }
      }
      if (Object.hasOwn(schema, "type")) {
        const types = Array.isArray(schema.type) ? schema.type : [schema.type];
        if (
          types.length === 0 ||
          types.some((type) => typeof type !== "string" || !JSON_SCHEMA_TYPES.has(type)) ||
          new Set(types).size !== types.length
        ) {
          schemaSubsetError(appendJsonPointer(schemaPath, "type"), "type must name unique supported JSON types");
        }
      }
      if (Object.hasOwn(schema, "const")) {
        constValues.set(
          schema,
          canonicalSchemaValue(schema.const, appendJsonPointer(schemaPath, "const")),
        );
      }
      if (Object.hasOwn(schema, "enum")) {
        if (!Array.isArray(schema.enum) || schema.enum.length === 0) {
          schemaSubsetError(appendJsonPointer(schemaPath, "enum"), "enum must be a non-empty array");
        }
        const values = schema.enum.map((value, index) =>
          canonicalSchemaValue(value, appendJsonPointer(appendJsonPointer(schemaPath, "enum"), String(index))),
        );
        if (new Set(values).size !== values.length) {
          schemaSubsetError(appendJsonPointer(schemaPath, "enum"), "enum values must be unique");
        }
        enumValues.set(schema, new Set(values));
      }
      if (Object.hasOwn(schema, "required")) {
        if (
          !Array.isArray(schema.required) ||
          schema.required.some((name) => typeof name !== "string") ||
          new Set(schema.required).size !== schema.required.length
        ) {
          schemaSubsetError(appendJsonPointer(schemaPath, "required"), "required must contain unique strings");
        }
      }
      if (Object.hasOwn(schema, "additionalProperties")) {
        if (typeof schema.additionalProperties === "boolean") {
          // Boolean additionalProperties is complete at this boundary.
        } else if (isJsonObject(schema.additionalProperties)) {
          visit(
            schema.additionalProperties,
            appendJsonPointer(schemaPath, "additionalProperties"),
            depth + 1,
          );
        } else {
          schemaSubsetError(
            appendJsonPointer(schemaPath, "additionalProperties"),
            "additionalProperties must be a boolean or schema object",
          );
        }
      }
      if (Object.hasOwn(schema, "items")) {
        visit(schema.items, appendJsonPointer(schemaPath, "items"), depth + 1);
      }
      for (const keyword of ["minItems", "maxItems", "minLength", "maxLength"]) {
        if (
          Object.hasOwn(schema, keyword) &&
          (!Number.isInteger(schema[keyword]) || schema[keyword] < 0)
        ) {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `${keyword} must be a non-negative integer`);
        }
      }
      if (
        Object.hasOwn(schema, "minItems") &&
        Object.hasOwn(schema, "maxItems") &&
        schema.minItems > schema.maxItems
      ) {
        schemaSubsetError(schemaPath, "minItems exceeds maxItems");
      }
      if (
        Object.hasOwn(schema, "minLength") &&
        Object.hasOwn(schema, "maxLength") &&
        schema.minLength > schema.maxLength
      ) {
        schemaSubsetError(schemaPath, "minLength exceeds maxLength");
      }
      if (Object.hasOwn(schema, "uniqueItems") && typeof schema.uniqueItems !== "boolean") {
        schemaSubsetError(appendJsonPointer(schemaPath, "uniqueItems"), "uniqueItems must be a boolean");
      }
      if (Object.hasOwn(schema, "pattern")) {
        if (typeof schema.pattern !== "string") {
          schemaSubsetError(appendJsonPointer(schemaPath, "pattern"), "pattern must be a string");
        }
        try {
          patterns.set(schema, new RegExp(schema.pattern, "u"));
        } catch (error) {
          schemaSubsetError(appendJsonPointer(schemaPath, "pattern"), `invalid pattern: ${error.message}`);
        }
      }
      for (const keyword of ["minimum", "maximum", "exclusiveMinimum", "exclusiveMaximum"]) {
        if (
          Object.hasOwn(schema, keyword) &&
          (typeof schema[keyword] !== "number" || !Number.isFinite(schema[keyword]))
        ) {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `${keyword} must be a finite number`);
        }
      }
      for (const keyword of ["oneOf", "allOf"]) {
        if (!Object.hasOwn(schema, keyword)) continue;
        if (!Array.isArray(schema[keyword]) || schema[keyword].length === 0) {
          schemaSubsetError(appendJsonPointer(schemaPath, keyword), `${keyword} must be a non-empty array`);
        }
        schema[keyword].forEach((child, index) => {
          visit(
            child,
            appendJsonPointer(appendJsonPointer(schemaPath, keyword), String(index)),
            depth + 1,
          );
        });
      }
      const hasIf = Object.hasOwn(schema, "if");
      const hasThen = Object.hasOwn(schema, "then");
      if (hasIf !== hasThen) {
        schemaSubsetError(schemaPath, "if and then must appear together in the supported subset");
      }
      if (hasIf) {
        visit(schema.if, appendJsonPointer(schemaPath, "if"), depth + 1);
        visit(schema.then, appendJsonPointer(schemaPath, "then"), depth + 1);
      }
    } finally {
      active.delete(schema);
    }
    complete.add(schema);
  }

  visit(rootSchema, "#", 0);
  return { references, patterns, constValues, enumValues };
}

function matchesJsonSchemaType(value, type) {
  switch (type) {
    case "null":
      return value === null;
    case "object":
      return isJsonObject(value);
    case "array":
      return Array.isArray(value);
    case "string":
      return typeof value === "string";
    case "boolean":
      return typeof value === "boolean";
    case "number":
      return typeof value === "number" && Number.isFinite(value);
    case "integer":
      return typeof value === "number" && Number.isFinite(value) && Number.isInteger(value);
    default:
      return false;
  }
}

function schemaValidationFailure(instancePath, schemaPath, message) {
  return { instancePath, schemaPath, message };
}

function canonicalInstanceValue(value) {
  try {
    const result = canonical(value);
    return typeof result === "string" ? result : undefined;
  } catch {
    return undefined;
  }
}

function validatePreparedJsonSchema(schema, value, prepared, instancePath, schemaPath, depth) {
  if (depth > JSON_SCHEMA_MAX_DEPTH) {
    return schemaValidationFailure(instancePath, schemaPath, "instance nesting exceeds its bound");
  }
  const resolved = prepared.references.get(schema);
  if (resolved) {
    const failure = validatePreparedJsonSchema(
      resolved.target,
      value,
      prepared,
      instancePath,
      resolved.schemaPath,
      depth + 1,
    );
    if (failure) return failure;
  }
  if (Object.hasOwn(schema, "type")) {
    const types = Array.isArray(schema.type) ? schema.type : [schema.type];
    if (!types.some((type) => matchesJsonSchemaType(value, type))) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "type"), `expected type ${types.join(" or ")}`);
    }
  }
  if (Object.hasOwn(schema, "const")) {
    if (canonicalInstanceValue(value) !== prepared.constValues.get(schema)) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "const"), "value does not equal const");
    }
  }
  if (Object.hasOwn(schema, "enum")) {
    const rendered = canonicalInstanceValue(value);
    if (rendered === undefined || !prepared.enumValues.get(schema).has(rendered)) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "enum"), "value is not in enum");
    }
  }
  if (isJsonObject(value)) {
    if (Object.hasOwn(schema, "required")) {
      for (const name of schema.required) {
        if (!Object.hasOwn(value, name)) {
          return schemaValidationFailure(
            instancePath,
            appendJsonPointer(schemaPath, "required"),
            `missing required property ${JSON.stringify(name)}`,
          );
        }
      }
    }
    const properties = schema.properties ?? {};
    for (const [name, child] of Object.entries(properties)) {
      if (!Object.hasOwn(value, name)) continue;
      const failure = validatePreparedJsonSchema(
        child,
        value[name],
        prepared,
        appendJsonPointer(instancePath, name),
        appendJsonPointer(appendJsonPointer(schemaPath, "properties"), name),
        depth + 1,
      );
      if (failure) return failure;
    }
    if (Object.hasOwn(schema, "additionalProperties")) {
      for (const name of Object.keys(value)) {
        if (Object.hasOwn(properties, name)) continue;
        if (schema.additionalProperties === false) {
          return schemaValidationFailure(
            appendJsonPointer(instancePath, name),
            appendJsonPointer(schemaPath, "additionalProperties"),
            `additional property ${JSON.stringify(name)} is not allowed`,
          );
        }
        if (isJsonObject(schema.additionalProperties)) {
          const failure = validatePreparedJsonSchema(
            schema.additionalProperties,
            value[name],
            prepared,
            appendJsonPointer(instancePath, name),
            appendJsonPointer(schemaPath, "additionalProperties"),
            depth + 1,
          );
          if (failure) return failure;
        }
      }
    }
  }
  if (Array.isArray(value)) {
    if (Object.hasOwn(schema, "minItems") && value.length < schema.minItems) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "minItems"), "array is shorter than minItems");
    }
    if (Object.hasOwn(schema, "maxItems") && value.length > schema.maxItems) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "maxItems"), "array is longer than maxItems");
    }
    if (schema.uniqueItems === true) {
      const seen = new Set();
      for (const item of value) {
        const rendered = canonicalInstanceValue(item);
        if (rendered === undefined) {
          return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "uniqueItems"), "array item is not JSON-compatible");
        }
        if (seen.has(rendered)) {
          return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "uniqueItems"), "array items are not unique");
        }
        seen.add(rendered);
      }
    }
    if (Object.hasOwn(schema, "items")) {
      for (let index = 0; index < value.length; index += 1) {
        const failure = validatePreparedJsonSchema(
          schema.items,
          value[index],
          prepared,
          appendJsonPointer(instancePath, String(index)),
          appendJsonPointer(schemaPath, "items"),
          depth + 1,
        );
        if (failure) return failure;
      }
    }
  }
  if (typeof value === "string") {
    const length = Array.from(value).length;
    if (Object.hasOwn(schema, "minLength") && length < schema.minLength) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "minLength"), "string is shorter than minLength");
    }
    if (Object.hasOwn(schema, "maxLength") && length > schema.maxLength) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "maxLength"), "string is longer than maxLength");
    }
    const pattern = prepared.patterns.get(schema);
    if (pattern && !pattern.test(value)) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "pattern"), "string does not match pattern");
    }
  }
  if (typeof value === "number" && Number.isFinite(value)) {
    if (Object.hasOwn(schema, "minimum") && value < schema.minimum) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "minimum"), "number is below minimum");
    }
    if (Object.hasOwn(schema, "maximum") && value > schema.maximum) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "maximum"), "number is above maximum");
    }
    if (Object.hasOwn(schema, "exclusiveMinimum") && value <= schema.exclusiveMinimum) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "exclusiveMinimum"), "number is not above exclusiveMinimum");
    }
    if (Object.hasOwn(schema, "exclusiveMaximum") && value >= schema.exclusiveMaximum) {
      return schemaValidationFailure(instancePath, appendJsonPointer(schemaPath, "exclusiveMaximum"), "number is not below exclusiveMaximum");
    }
  }
  if (Object.hasOwn(schema, "allOf")) {
    for (let index = 0; index < schema.allOf.length; index += 1) {
      const failure = validatePreparedJsonSchema(
        schema.allOf[index],
        value,
        prepared,
        instancePath,
        appendJsonPointer(appendJsonPointer(schemaPath, "allOf"), String(index)),
        depth + 1,
      );
      if (failure) return failure;
    }
  }
  if (Object.hasOwn(schema, "oneOf")) {
    let matches = 0;
    for (let index = 0; index < schema.oneOf.length; index += 1) {
      const failure = validatePreparedJsonSchema(
        schema.oneOf[index],
        value,
        prepared,
        instancePath,
        appendJsonPointer(appendJsonPointer(schemaPath, "oneOf"), String(index)),
        depth + 1,
      );
      if (!failure) matches += 1;
    }
    if (matches !== 1) {
      return schemaValidationFailure(
        instancePath,
        appendJsonPointer(schemaPath, "oneOf"),
        `oneOf matched ${matches} branches instead of exactly one`,
      );
    }
  }
  if (Object.hasOwn(schema, "if")) {
    const conditionFailure = validatePreparedJsonSchema(
      schema.if,
      value,
      prepared,
      instancePath,
      appendJsonPointer(schemaPath, "if"),
      depth + 1,
    );
    if (!conditionFailure) {
      const failure = validatePreparedJsonSchema(
        schema.then,
        value,
        prepared,
        instancePath,
        appendJsonPointer(schemaPath, "then"),
        depth + 1,
      );
      if (failure) return failure;
    }
  }
  return undefined;
}

export function validateJsonSchemaSubset(schema, value, label = "JSON value") {
  const prepared = prepareJsonSchemaSubset(schema);
  const issue = findJsonValueIssue(value);
  if (issue) {
    throw new Error(
      `${label} violates JSON schema at instance ${displayJsonPointer(issue.valuePath)} ` +
        `against /: ${issue.message}`,
    );
  }
  const failure = validatePreparedJsonSchema(schema, value, prepared, "", "#", 0);
  if (failure) {
    throw new Error(
      `${label} violates JSON schema at instance ${displayJsonPointer(failure.instancePath)} ` +
        `against ${displayJsonPointer(failure.schemaPath)}: ${failure.message}`,
    );
  }
  return value;
}

function readWorkspaceJson(relativePath, label) {
  try {
    return JSON.parse(fs.readFileSync(path.join(workspace, relativePath), "utf8"));
  } catch (error) {
    throw new Error(`${label} is not valid JSON: ${error.message}`);
  }
}

export function validateArtifactSchemaContracts() {
  for (const contract of ARTIFACT_SCHEMA_CONTRACTS) {
    const schema = readWorkspaceJson(contract.schema, `${contract.label} schema`);
    const artifact = readWorkspaceJson(contract.artifact, `${contract.label} artifact`);
    validateJsonSchemaSubset(schema, artifact, `${contract.label} artifact`);
  }
  return ARTIFACT_SCHEMA_CONTRACTS;
}

const SLICE_READINESS_CHECKER = ".github/ci/slice-readiness.mjs";
const SLICE_READINESS_SCHEMA = ".github/ci/contracts/slice-readiness.v1.schema.json";
const FCI_READINESS_DIR = "ratchets/fci-readiness";

function runSliceReadinessChecker(args, label) {
  try {
    execFileSync(process.execPath, [path.join(workspace, SLICE_READINESS_CHECKER), ...args], {
      cwd: workspace,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch (error) {
    const detail = `${error.stdout ?? ""}${error.stderr ?? ""}`.trim() || error.message;
    throw new Error(`${label}: ${detail}`);
  }
}

// The packet-control bootstrap names this policy entry point as the caller of
// the slice-readiness checker. Envelope-only edits are additionally validated
// by their packet's own recorded proof commands; this chain check makes every
// policy run revalidate the bootstrap record, every envelope's schema and
// identity, and the packet digest of every `ready` packet.
export function validateFciReadinessChain() {
  const schema = readWorkspaceJson(SLICE_READINESS_SCHEMA, "slice-readiness schema");
  runSliceReadinessChecker(["--bootstrap-check"], "packet-control bootstrap record");
  const names = fs
    .readdirSync(path.join(workspace, FCI_READINESS_DIR))
    .filter((name) => name.endsWith(".v1.json"))
    .sort();
  if (names.length === 0) {
    throw new Error("fci readiness: no envelopes found under ratchets/fci-readiness");
  }
  const summary = { envelopes: 0, ready: 0 };
  for (const name of names) {
    const label = `fci readiness envelope ${name}`;
    const envelope = readWorkspaceJson(`${FCI_READINESS_DIR}/${name}`, label);
    validateJsonSchemaSubset(schema, envelope, label);
    if (`${envelope.packetId}.v1.json` !== name) {
      throw new Error(`${label}: packetId ${envelope.packetId} does not match its file name`);
    }
    summary.envelopes += 1;
    if (envelope.status === "ready") {
      runSliceReadinessChecker(["--check", envelope.packetId], label);
      summary.ready += 1;
    }
  }
  return summary;
}

function rustBoundaryError(label, message) {
  throw new Error(`${label}: ${message}`);
}

function blankRustRange(output, source, start, end) {
  for (let index = start; index < end; index += 1) {
    if (source[index] !== "\n" && source[index] !== "\r") output[index] = " ";
  }
}

function rawRustStringAt(source, start, label) {
  let rawMarker = start;
  if ((source[start] === "b" || source[start] === "c") && source[start + 1] === "r") {
    rawMarker += 1;
  } else if (source[start] !== "r") {
    return undefined;
  }
  if (start > 0 && /[A-Za-z0-9_]/u.test(source[start - 1])) return undefined;
  let quote = rawMarker + 1;
  while (source[quote] === "#") quote += 1;
  if (source[quote] !== '"') return undefined;
  const hashCount = quote - rawMarker - 1;
  if (hashCount > 255) rustBoundaryError(label, "raw string delimiter exceeds Rust's bound");
  const terminator = `"${"#".repeat(hashCount)}`;
  const close = source.indexOf(terminator, quote + 1);
  if (close < 0) rustBoundaryError(label, "unterminated raw string literal");
  return close + terminator.length;
}

function quotedRustStringEnd(source, start, label) {
  let cursor = start + 1;
  while (cursor < source.length) {
    if (source[cursor] === "\\") {
      cursor += 2;
      continue;
    }
    if (source[cursor] === '"') return cursor + 1;
    cursor += 1;
  }
  rustBoundaryError(label, "unterminated quoted string literal");
}

function rustCharacterEnd(source, start) {
  let cursor = start + 1;
  if (cursor >= source.length || source[cursor] === "\n" || source[cursor] === "\r") {
    return undefined;
  }
  if (source[cursor] === "\\") {
    cursor += 1;
    if (source[cursor] === "u" && source[cursor + 1] === "{") {
      const brace = source.indexOf("}", cursor + 2);
      if (brace < 0) return undefined;
      cursor = brace + 1;
    } else if (source[cursor] === "x") {
      cursor += 3;
    } else {
      cursor += 1;
    }
  } else {
    const codePoint = source.codePointAt(cursor);
    cursor += codePoint > 0xffff ? 2 : 1;
  }
  return source[cursor] === "'" ? cursor + 1 : undefined;
}

function sanitizeRustSource(source, label) {
  if (typeof source !== "string") rustBoundaryError(label, "Rust source must be a string");
  if (Buffer.byteLength(source, "utf8") > RUST_SOURCE_MAX_BYTES) {
    rustBoundaryError(label, "Rust source exceeds its byte bound");
  }
  const output = source.split("");
  let cursor = 0;
  while (cursor < source.length) {
    if (source.startsWith("//", cursor)) {
      let end = source.indexOf("\n", cursor + 2);
      if (end < 0) end = source.length;
      blankRustRange(output, source, cursor, end);
      cursor = end;
      continue;
    }
    if (source.startsWith("/*", cursor)) {
      let nesting = 1;
      let end = cursor + 2;
      while (end < source.length && nesting > 0) {
        if (source.startsWith("/*", end)) {
          nesting += 1;
          if (nesting > RUST_LEXICAL_NESTING_MAX) {
            rustBoundaryError(label, "block-comment nesting exceeds its bound");
          }
          end += 2;
        } else if (source.startsWith("*/", end)) {
          nesting -= 1;
          end += 2;
        } else {
          end += 1;
        }
      }
      if (nesting !== 0) rustBoundaryError(label, "unterminated block comment");
      blankRustRange(output, source, cursor, end);
      cursor = end;
      continue;
    }
    const rawEnd = rawRustStringAt(source, cursor, label);
    if (rawEnd !== undefined) {
      blankRustRange(output, source, cursor, rawEnd);
      cursor = rawEnd;
      continue;
    }
    if (source[cursor] === '"') {
      const end = quotedRustStringEnd(source, cursor, label);
      blankRustRange(output, source, cursor, end);
      cursor = end;
      continue;
    }
    if (source[cursor] === "'") {
      const end = rustCharacterEnd(source, cursor);
      if (end !== undefined) {
        blankRustRange(output, source, cursor, end);
        cursor = end;
        continue;
      }
    }
    cursor += 1;
  }
  return output.join("");
}

function regexEscape(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&");
}

function matchingRustDelimiter(source, openIndex, open, close, label) {
  let nesting = 0;
  for (let cursor = openIndex; cursor < source.length; cursor += 1) {
    if (source[cursor] === open) {
      nesting += 1;
      if (nesting > RUST_LEXICAL_NESTING_MAX) {
        rustBoundaryError(label, `${open}${close} nesting exceeds its bound`);
      }
    } else if (source[cursor] === close) {
      nesting -= 1;
      if (nesting === 0) return cursor;
      if (nesting < 0) break;
    }
  }
  rustBoundaryError(label, `unmatched ${open} in Rust source`);
}

function extractRustFunctionBody(sanitized, functionName, label) {
  const declaration = new RegExp(
    `^[\\t ]*(?:pub(?:\\([^\\n)]*\\))?[\\t ]+)?fn[\\t ]+${regexEscape(functionName)}[\\t \\r\\n]*\\(`,
    "gmu",
  );
  const matches = [...sanitized.matchAll(declaration)];
  if (matches.length !== 1) {
    rustBoundaryError(label, `expected exactly one ${functionName} function, found ${matches.length}`);
  }
  const match = matches[0];
  const parametersStart = match.index + match[0].lastIndexOf("(");
  const parametersEnd = matchingRustDelimiter(
    sanitized,
    parametersStart,
    "(",
    ")",
    `${label} ${functionName}`,
  );
  const signatureBound = Math.min(sanitized.length, parametersEnd + 65_536);
  let bodyStart = parametersEnd + 1;
  while (bodyStart < signatureBound && sanitized[bodyStart] !== "{") {
    if (sanitized[bodyStart] === ";") {
      rustBoundaryError(label, `${functionName} is a declaration without a body`);
    }
    bodyStart += 1;
  }
  if (bodyStart >= signatureBound) rustBoundaryError(label, `${functionName} body was not found within its bound`);
  const bodyEnd = matchingRustDelimiter(
    sanitized,
    bodyStart,
    "{",
    "}",
    `${label} ${functionName}`,
  );
  return sanitized.slice(bodyStart + 1, bodyEnd);
}

function rustCallSuffix(body, closeIndex) {
  let cursor = closeIndex + 1;
  while (/\s/u.test(body[cursor] ?? "")) cursor += 1;
  if (body[cursor] === "?") {
    cursor += 1;
    while (/\s/u.test(body[cursor] ?? "")) cursor += 1;
    if (body[cursor] === ";") return "?;";
    return "expression";
  }
  if (body[cursor] === ";") return ";";
  return body.slice(cursor).trim().length === 0 ? "tail" : "expression";
}

function extractRustCalls(body, label) {
  const pattern = /\b([A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)\s*\(/gu;
  const keywords = new Set(["if", "while", "for", "match", "loop", "return"]);
  const calls = [];
  let braceCursor = 0;
  let braceDepth = 0;
  for (const match of body.matchAll(pattern)) {
    while (braceCursor < match.index) {
      if (body[braceCursor] === "{") braceDepth += 1;
      if (body[braceCursor] === "}") braceDepth -= 1;
      if (braceDepth < 0 || braceDepth > RUST_LEXICAL_NESTING_MAX) {
        rustBoundaryError(label, "invalid call-statement brace nesting");
      }
      braceCursor += 1;
    }
    if (keywords.has(match[1])) continue;
    const openIndex = match.index + match[0].lastIndexOf("(");
    const closeIndex = matchingRustDelimiter(body, openIndex, "(", ")", label);
    let previous = match.index - 1;
    while (previous >= 0 && /\s/u.test(body[previous])) previous -= 1;
    calls.push({
      callee: match[1],
      arguments: body.slice(openIndex + 1, closeIndex).replaceAll(/\s+/gu, ""),
      method: body[previous] === ".",
      topLevel: braceDepth === 0,
      suffix: rustCallSuffix(body, closeIndex),
    });
  }
  return calls;
}

function containsRustIdentifier(body, identifier) {
  return new RegExp(`\\b${regexEscape(identifier)}\\b`, "u").test(body);
}

function containsRustOwnerControlIdentifier(body) {
  return /\b[A-Za-z_][A-Za-z0-9_]*owner_controls?[A-Za-z0-9_]*\b/iu.test(body);
}

function rustModuleName(modulePath) {
  return path.posix.basename(modulePath, ".rs");
}

function extractTopLevelRustFunctions(sanitized, moduleName, label) {
  const declaration = /^[\t ]*(?:pub(?:\([^\n)]*\))?[\t ]+)?(?:(?:const|async|unsafe)[\t ]+)*(?:extern[\t ]+)?fn[\t ]+([A-Za-z_][A-Za-z0-9_]*)\b/gmu;
  const functions = new Map();
  let braceCursor = 0;
  let braceDepth = 0;
  for (const match of sanitized.matchAll(declaration)) {
    while (braceCursor < match.index) {
      if (sanitized[braceCursor] === "{") braceDepth += 1;
      if (sanitized[braceCursor] === "}") braceDepth -= 1;
      if (braceDepth < 0 || braceDepth > RUST_LEXICAL_NESTING_MAX) {
        rustBoundaryError(label, "invalid top-level function brace nesting");
      }
      braceCursor += 1;
    }
    if (braceDepth !== 0) continue;
    const parametersBound = Math.min(sanitized.length, match.index + match[0].length + 65_536);
    const parametersStart = sanitized.indexOf("(", match.index + match[0].length);
    if (parametersStart < 0 || parametersStart >= parametersBound) {
      rustBoundaryError(label, `${match[1]} parameters were not found within their bound`);
    }
    const parametersEnd = matchingRustDelimiter(
      sanitized,
      parametersStart,
      "(",
      ")",
      `${label} ${match[1]}`,
    );
    const signatureBound = Math.min(sanitized.length, parametersEnd + 65_536);
    let bodyStart = parametersEnd + 1;
    while (bodyStart < signatureBound && sanitized[bodyStart] !== "{") {
      bodyStart += 1;
    }
    if (bodyStart >= signatureBound) {
      rustBoundaryError(label, `${match[1]} body was not found within its bound`);
    }
    const bodyEnd = matchingRustDelimiter(
      sanitized,
      bodyStart,
      "{",
      "}",
      `${label} ${match[1]}`,
    );
    if (functions.has(match[1])) {
      rustBoundaryError(label, `duplicate top-level function ${match[1]} is ambiguous`);
    }
    functions.set(match[1], {
      body: sanitized.slice(bodyStart + 1, bodyEnd),
      functionName: match[1],
      label,
      moduleName,
    });
  }
  return functions;
}

function addRustAlias(aliases, alias, target, label) {
  if (alias === "_") return;
  if (aliases.has(alias) && aliases.get(alias) !== target) {
    rustBoundaryError(label, `Rust alias ${alias} has ambiguous targets`);
  }
  aliases.set(alias, target);
}

function extractRustUseAliases(sanitized, label) {
  const aliases = new Map();
  const pattern = /\buse\s+((?:crate|self|super|[A-Za-z_][A-Za-z0-9_]*)(?:::[A-Za-z_][A-Za-z0-9_]*)+)\s+as\s+([A-Za-z_][A-Za-z0-9_]*|_)\s*;/gu;
  for (const match of sanitized.matchAll(pattern)) {
    addRustAlias(aliases, match[2], match[1], label);
  }
  const grouped = /\buse\s+((?:crate|self|super|[A-Za-z_][A-Za-z0-9_]*)(?:::[A-Za-z_][A-Za-z0-9_]*)*)::\{([^{}]*)\}\s*;/gu;
  for (const match of sanitized.matchAll(grouped)) {
    for (const rawItem of match[2].split(",")) {
      const item = rawItem.trim();
      if (item.length === 0 || item === "*") continue;
      const parsed = /^((?:self|[A-Za-z_][A-Za-z0-9_]*)(?:::[A-Za-z_][A-Za-z0-9_]*)*)(?:\s+as\s+([A-Za-z_][A-Za-z0-9_]*|_))?$/u.exec(item);
      if (!parsed) rustBoundaryError(label, `unsupported grouped Rust alias item ${item}`);
      const target = parsed[1] === "self" ? match[1] : `${match[1]}::${parsed[1]}`;
      const alias = parsed[2] ?? parsed[1].split("::").at(-1);
      addRustAlias(aliases, alias, target, label);
    }
  }
  return aliases;
}

function extractRustLetAliases(body, label) {
  const aliases = new Map();
  const pattern = /\blet\s+(?:mut\s+)?([A-Za-z_][A-Za-z0-9_]*)(?:\s*:[^=;]+)?\s*=\s*\(?\s*((?:crate|self|super|[A-Za-z_][A-Za-z0-9_]*)(?:::[A-Za-z_][A-Za-z0-9_]*)*)\s*\)?\s*;/gu;
  for (const match of body.matchAll(pattern)) {
    addRustAlias(aliases, match[1], match[2], label);
  }
  return aliases;
}

function rustFunctionKey(moduleName, functionName) {
  return `${moduleName}::${functionName}`;
}

function extractRustModuleNames(sanitized) {
  return new Set(
    [...sanitized.matchAll(/^[\t ]*(?:pub(?:\([^\n)]*\))?[\t ]+)?mod[\t ]+([A-Za-z_][A-Za-z0-9_]*)[\t ]*;/gmu)]
      .map((match) => match[1]),
  );
}

function resolveRustFunctionPath(rawPath, currentModule, tables, localModules, aliases, label) {
  let callable = rawPath;
  const expanded = new Set();
  let usedAlias = false;
  for (;;) {
    const firstSeparator = callable.indexOf("::");
    const first = firstSeparator < 0 ? callable : callable.slice(0, firstSeparator);
    if (!aliases.has(first)) break;
    if (expanded.has(first)) rustBoundaryError(label, `cyclic Rust alias ${first}`);
    expanded.add(first);
    usedAlias = true;
    const suffix = firstSeparator < 0 ? "" : callable.slice(firstSeparator);
    callable = `${aliases.get(first)}${suffix}`;
  }

  const segments = callable.split("::");
  if (segments.length === 1) {
    return tables.get(currentModule)?.has(segments[0])
      ? rustFunctionKey(currentModule, segments[0])
      : undefined;
  }

  const knownModules = new Set(tables.keys());
  const functionName = segments.at(-1);
  let targetModule;
  if (segments[0] === "self") {
    targetModule = currentModule;
  } else if (segments[0] === "super" && segments.length === 2) {
    targetModule = "main";
  } else if (segments[0] === "crate" && segments.length === 2) {
    targetModule = "main";
  } else {
    targetModule = segments.find((segment) => knownModules.has(segment));
  }
  if (targetModule !== undefined) {
    if (!tables.get(targetModule)?.has(functionName)) {
      rustBoundaryError(label, `unresolved local Rust function path ${callable}`);
    }
    return rustFunctionKey(targetModule, functionName);
  }
  if (["self", "super", "crate"].includes(segments[0])) {
    const kind = usedAlias ? "alias target" : "function path";
    rustBoundaryError(label, `unresolved local Rust ${kind} ${callable}`);
  }
  if (localModules.has(segments[0])) {
    rustBoundaryError(label, `unresolved local Rust module path ${callable}`);
  }
  return undefined;
}

function validateHostedRustCallGraph(sanitizedXtask, sanitizedModules) {
  const sources = new Map([
    ["main", { label: "crates/xtask/src/main.rs", source: sanitizedXtask }],
  ]);
  for (const [modulePath, source] of Object.entries(sanitizedModules)) {
    const moduleName = rustModuleName(modulePath);
    if (sources.has(moduleName)) {
      rustBoundaryError(modulePath, `duplicate Rust module name ${moduleName}`);
    }
    sources.set(moduleName, { label: modulePath, source });
  }

  const tables = new Map();
  const localModules = extractRustModuleNames(sanitizedXtask);
  const useAliases = new Map();
  const functions = new Map();
  for (const [moduleName, source] of sources) {
    const table = extractTopLevelRustFunctions(source.source, moduleName, source.label);
    tables.set(moduleName, table);
    useAliases.set(moduleName, extractRustUseAliases(source.source, source.label));
    for (const record of table.values()) {
      functions.set(rustFunctionKey(moduleName, record.functionName), record);
    }
  }

  const root = rustFunctionKey("main", "acceptance");
  if (!functions.has(root)) rustBoundaryError("hosted call graph", "acceptance root is missing");
  const pending = [root];
  const visited = new Set();
  while (pending.length > 0) {
    const key = pending.pop();
    if (visited.has(key)) continue;
    visited.add(key);
    if (visited.size > 4096) rustBoundaryError("hosted call graph", "reachable function bound exceeded");
    const record = functions.get(key);
    if (!record) rustBoundaryError("hosted call graph", `reachable function ${key} is unresolved`);
    if (
      containsRustOwnerControlIdentifier(record.functionName) ||
      containsRustOwnerControlIdentifier(record.body)
    ) {
      rustBoundaryError(
        "hosted call graph",
        `reachable function ${key} contains an owner-control symbol or path`,
      );
    }

    const aliases = new Map(useAliases.get(record.moduleName));
    for (const [alias, target] of extractRustLetAliases(record.body, key)) {
      addRustAlias(aliases, alias, target, key);
      const targetKey = resolveRustFunctionPath(
        target,
        record.moduleName,
        tables,
        localModules,
        aliases,
        key,
      );
      if (targetKey) pending.push(targetKey);
    }
    for (const call of extractRustCalls(record.body, key)) {
      if (call.method) continue;
      const target = resolveRustFunctionPath(
        call.callee,
        record.moduleName,
        tables,
        localModules,
        aliases,
        key,
      );
      if (target) pending.push(target);
    }
  }
  return visited;
}

function validateHostedRustSourcePins(
  xtaskSource,
  moduleSources,
  sourceSha256,
  rawSourceBytes,
) {
  const sources = rawSourceBytes ?? {
    "crates/xtask/src/main.rs": xtaskSource,
    ...moduleSources,
  };
  if (
    !isJsonObject(sourceSha256) ||
    canonical(Object.keys(sourceSha256).sort()) !== canonical(Object.keys(sources).sort())
  ) {
    rustBoundaryError("hosted Rust source pins", "source path set drifted");
  }
  // Hash the complete raw UTF-8 files, not lexer output. This is intentionally
  // parser-independent: helper, macro, impl, nested-module, alias, attribute,
  // comment, and literal changes all require an explicit policy pin update.
  for (const [sourcePath, source] of Object.entries(sources)) {
    if (typeof source !== "string" && !Buffer.isBuffer(source)) {
      rustBoundaryError("hosted Rust source pins", `${sourcePath} is not raw source bytes`);
    }
    if (!isSha256(sourceSha256[sourcePath])) {
      rustBoundaryError("hosted Rust source pins", `${sourcePath} has an invalid SHA-256 pin`);
    }
    if (sha256(source) !== sourceSha256[sourcePath]) {
      rustBoundaryError("hosted Rust source pins", `${sourcePath} content hash drifted`);
    }
  }
}

function rustBraceDepthAt(sanitized, end, label) {
  let depth = 0;
  for (let index = 0; index < end; index += 1) {
    if (sanitized[index] === "{") depth += 1;
    if (sanitized[index] === "}") depth -= 1;
    if (depth < 0 || depth > RUST_LEXICAL_NESTING_MAX) {
      rustBoundaryError(label, "invalid module-declaration brace nesting");
    }
  }
  return depth;
}

function validateHostedRustModuleDeclarations(sanitizedXtask) {
  const indexes = [];
  for (const modulePath of HOSTED_ACCEPTANCE_MODULES) {
    const moduleName = rustModuleName(modulePath);
    const pattern = new RegExp(`\\bmod[\\t ]+${regexEscape(moduleName)}\\b`, "gu");
    const matches = [...sanitizedXtask.matchAll(pattern)];
    if (matches.length !== 1) {
      rustBoundaryError(
        "crates/xtask/src/main.rs",
        `expected exactly one canonical ${moduleName} module declaration`,
      );
    }
    const match = matches[0];
    const lineStart = sanitizedXtask.lastIndexOf("\n", match.index) + 1;
    let lineEnd = sanitizedXtask.indexOf("\n", match.index);
    if (lineEnd < 0) lineEnd = sanitizedXtask.length;
    if (sanitizedXtask.slice(lineStart, lineEnd).trim() !== `mod ${moduleName};`) {
      rustBoundaryError(
        "crates/xtask/src/main.rs",
        `${moduleName} must use the canonical mod name; declaration`,
      );
    }
    if (rustBraceDepthAt(sanitizedXtask, match.index, "crates/xtask/src/main.rs") !== 0) {
      rustBoundaryError(
        "crates/xtask/src/main.rs",
        `${moduleName} declaration must remain top-level`,
      );
    }
    const priorLines = sanitizedXtask.slice(0, lineStart).split(/\r?\n/u);
    while (priorLines.length > 0 && priorLines.at(-1).trim().length === 0) priorLines.pop();
    if (priorLines.at(-1)?.trim().startsWith("#[")) {
      rustBoundaryError(
        "crates/xtask/src/main.rs",
        `${moduleName} declaration must not have cfg/path attributes`,
      );
    }
    indexes.push(match.index);
  }
  if (indexes.some((index, position) => position > 0 && index <= indexes[position - 1])) {
    rustBoundaryError("crates/xtask/src/main.rs", "hosted module declaration order drifted");
  }
}

export function validateRustOwnerControlBoundaries({
  xtaskSource,
  moduleSources,
  sourceSha256,
  rawSourceBytes,
}) {
  if (!isJsonObject(moduleSources)) rustBoundaryError("H2 owner-control boundary", "moduleSources must be an object");
  if (
    canonical(Object.keys(moduleSources).sort()) !== canonical([...HOSTED_ACCEPTANCE_MODULES].sort())
  ) {
    rustBoundaryError("H2 owner-control boundary", "hosted acceptance module set drifted");
  }
  const sanitizedXtask = sanitizeRustSource(xtaskSource, "crates/xtask/src/main.rs");
  const sanitizedModules = Object.fromEntries(
    HOSTED_ACCEPTANCE_MODULES.map((modulePath) => [
      modulePath,
      sanitizeRustSource(moduleSources[modulePath], modulePath),
    ]),
  );
  validateHostedRustModuleDeclarations(sanitizedXtask);
  const acceptanceBody = extractRustFunctionBody(
    sanitizedXtask,
    "acceptance",
    "crates/xtask/src/main.rs",
  );
  if (acceptanceBody.replaceAll(/\s+/gu, "") !== HOSTED_ACCEPTANCE_CANONICAL_BODY) {
    rustBoundaryError(
      "hosted acceptance function",
      "body differs from the pinned canonical ts-tests entrypoint",
    );
  }
  const acceptanceCalls = extractRustCalls(acceptanceBody, "hosted acceptance function");
  if (
    acceptanceCalls.some((call) => call.callee.includes("owner_control")) ||
    containsRustOwnerControlIdentifier(acceptanceBody) ||
    containsRustIdentifier(acceptanceBody, "OWNER_CONTROLS_RELATIVE_PATH")
  ) {
    rustBoundaryError("hosted acceptance function", "contains a direct owner-control call or input");
  }
  if (
    !acceptanceCalls.some(
      (call) =>
        call.callee === "h2_2c_acceptance::run_h2_5g" &&
        call.arguments === "&workspace" &&
        call.topLevel,
    )
  ) {
    rustBoundaryError("hosted acceptance function", "does not call the qualified H2.5g runner");
  }
  const qualifiedAcceptanceCalls = acceptanceCalls
    .filter((call) => !call.method && call.callee.includes("::"))
    .map((call) => call.callee);
  if (canonical(qualifiedAcceptanceCalls) !== canonical(HOSTED_ACCEPTANCE_QUALIFIED_CALLS)) {
    rustBoundaryError(
      "hosted acceptance function",
      "qualified call sequence differs from the pinned ts-tests entrypoint",
    );
  }

  for (const modulePath of H2_OWNER_SPLIT_MODULES) {
    const sanitized = sanitizedModules[modulePath];
    const runBody = extractRustFunctionBody(sanitized, "run", modulePath);
    extractRustFunctionBody(sanitized, "run_owner_controls", modulePath);
    const runCalls = extractRustCalls(runBody, `${modulePath} run function`);
    if (
      runCalls.some((call) => call.callee.includes("owner_control")) ||
      containsRustOwnerControlIdentifier(runBody) ||
      containsRustIdentifier(runBody, "OWNER_CONTROLS_RELATIVE_PATH")
    ) {
      rustBoundaryError(modulePath, "hosted run function contains a direct owner-control call or input");
    }
  }
  validateHostedRustCallGraph(sanitizedXtask, sanitizedModules);

  const localBody = extractRustFunctionBody(
    sanitizedXtask,
    "ci_h2_owner_controls",
    "crates/xtask/src/main.rs",
  );
  const expectedLocalBody = H2_LOCAL_OWNER_CALLS.map((callee, index) =>
    index + 1 === H2_LOCAL_OWNER_CALLS.length
      ? `${callee}(workspace)`
      : `${callee}(workspace)?;`,
  ).join("");
  if (localBody.replaceAll(/\s+/gu, "") !== expectedLocalBody) {
    rustBoundaryError(
      "ci_h2_owner_controls",
      "body differs from the exact complete local owner-control statements",
    );
  }
  const localQualifiedCalls = extractRustCalls(localBody, "ci_h2_owner_controls")
    .filter((call) => call.callee.includes("::"));
  if (
    canonical(localQualifiedCalls.map((call) => call.callee)) !== canonical(H2_LOCAL_OWNER_CALLS)
  ) {
    rustBoundaryError(
      "ci_h2_owner_controls",
      "qualified owner-control call sequence differs from the complete local gate",
    );
  }
  for (let index = 0; index < localQualifiedCalls.length; index += 1) {
    const call = localQualifiedCalls[index];
    const expectedSuffix = index + 1 === localQualifiedCalls.length ? "tail" : "?;";
    if (!call.topLevel || call.arguments !== "workspace" || call.suffix !== expectedSuffix) {
      rustBoundaryError(
        "ci_h2_owner_controls",
        `qualified call ${call.callee} is not an exact top-level local-gate statement`,
      );
    }
  }
  validateHostedRustSourcePins(
    xtaskSource,
    moduleSources,
    sourceSha256,
    rawSourceBytes,
  );
  return true;
}

function sortedUnique(values) {
  return [...new Set(values)].sort((left, right) => left.localeCompare(right));
}

export function pathsDigest(paths) {
  return sha256(Buffer.from(paths.join("\0"), "utf8"));
}

export function validateLaneSelection(selection, limit = 4096) {
  const required = [
    "schema",
    "kind",
    "head_sha",
    "base_sha",
    "paths_sha256",
    "changed_paths",
    "docs_only",
    "selected",
  ];
  if (!exactKeys(selection, required)) throw new Error("lane selection has missing or unknown fields");
  if (selection.schema !== 1 || selection.kind !== "lane-selection") throw new Error("invalid lane selection discriminator");
  if (!isSha1(selection.head_sha) || !isSha1(selection.base_sha)) throw new Error("invalid lane selection commit");
  if (!Array.isArray(selection.changed_paths) || selection.changed_paths.length > limit) {
    throw new Error("lane selection path list exceeds its bound");
  }
  if (selection.changed_paths.some((entry) => !isRelativeRepositoryPath(entry))) {
    throw new Error("lane selection contains an invalid repository path");
  }
  if (JSON.stringify(selection.changed_paths) !== JSON.stringify(sortedUnique(selection.changed_paths))) {
    throw new Error("lane selection paths must be sorted and unique");
  }
  if (!isSha256(selection.paths_sha256) || selection.paths_sha256 !== pathsDigest(selection.changed_paths)) {
    throw new Error("lane selection path digest mismatch");
  }
  if (typeof selection.docs_only !== "boolean") throw new Error("lane selection docs_only is not boolean");
  if (!exactKeys(selection.selected, ["static", "host_platform", "program_path", "tracks"])) {
    throw new Error("lane selection selected object has missing or unknown fields");
  }
  for (const key of ["static", "host_platform", "program_path"]) {
    if (typeof selection.selected[key] !== "boolean") throw new Error(`lane selection ${key} is not boolean`);
  }
  const tracks = selection.selected.tracks;
  if (!Array.isArray(tracks) || JSON.stringify(tracks) !== JSON.stringify(sortedUnique(tracks))) {
    throw new Error("lane selection tracks must be sorted and unique");
  }
  if (tracks.some((track) => !["common", "h1", "l0-l1"].includes(track))) {
    throw new Error("lane selection contains an unknown track");
  }
  if (selection.docs_only) {
    if (selection.changed_paths.length === 0 || selection.selected.static || selection.selected.host_platform || selection.selected.program_path || tracks.length > 0) {
      throw new Error("documentation-only selection must select no execution lane");
    }
  } else if (!selection.selected.static || !tracks.includes("common")) {
    throw new Error("non-documentation selection must include static/common validation");
  }
  if (selection.selected.program_path && !selection.selected.host_platform) {
    throw new Error("program-path selection requires host-platform validation");
  }
  return selection;
}

export function classifyPaths({ paths, headSha, baseSha, statusBlockEqual, policy }) {
  const changedPaths = sortedUnique(paths);
  if (changedPaths.length > policy.limits.changed_paths) throw new Error("changed-path inventory exceeds policy bound");
  const docsOnly =
    changedPaths.length > 0 &&
    changedPaths.every((entry) => entry.endsWith(".md")) &&
    statusBlockEqual;
  let hostPlatform = false;
  let programPath = false;
  const tracks = new Set();
  if (!docsOnly) {
    tracks.add("common");
    for (const changedPath of changedPaths) {
      const hostMatch = policy.classification.host_platform_prefixes.some((prefix) => changedPath.startsWith(prefix));
      const programMatch = policy.classification.program_path_prefixes.some((prefix) => changedPath.startsWith(prefix));
      const commonMatch =
        policy.classification.common_exact.includes(changedPath) ||
        policy.classification.common_prefixes.some((prefix) => changedPath.startsWith(prefix));
      hostPlatform ||= hostMatch;
      programPath ||= programMatch;
      let known = hostMatch || programMatch || commonMatch;
      if (commonMatch) {
        tracks.add("l0-l1");
        tracks.add("h1");
      }
      for (const [track, prefixes] of Object.entries(policy.classification.track_prefixes)) {
        if (prefixes.some((prefix) => changedPath.startsWith(prefix))) {
          tracks.add(track);
          known = true;
        }
      }
      if (!known) {
        tracks.add("l0-l1");
        tracks.add("h1");
        hostPlatform = true;
        programPath = true;
      }
    }
  }
  return validateLaneSelection(
    {
      schema: 1,
      kind: "lane-selection",
      head_sha: headSha,
      base_sha: baseSha,
      paths_sha256: pathsDigest(changedPaths),
      changed_paths: changedPaths,
      docs_only: docsOnly,
      selected: {
        static: !docsOnly,
        host_platform: hostPlatform,
        program_path: programPath,
        tracks: sortedUnique([...tracks]),
      },
    },
    policy.limits.changed_paths,
  );
}

export function receiptResultHash(receipt) {
  const semantic = { ...receipt };
  delete semantic.result_sha256;
  delete semantic.authentication;
  return sha256(canonical(semantic));
}

export function qualificationResultHash(result) {
  const semantic = { ...result };
  delete semantic.result_sha256;
  return sha256(canonical(semantic));
}

export function validateQualificationResult(result) {
  const required = ["schema", "kind", "head_sha", "base_sha", "inputs", "lanes", "commands", "result_sha256"];
  if (!exactKeys(result, required)) throw new Error("qualification result has missing or unknown fields");
  const receiptShape = {
    ...result,
    kind: "exact-merge-qualification",
    authentication: {
      kind: "trusted-runner-oidc",
      issuer: "pending",
      subject: "pending",
      attestation_sha256: "0".repeat(64),
    },
  };
  receiptShape.result_sha256 = receiptResultHash(receiptShape);
  validateMergeReceipt(receiptShape);
  if (result.schema !== 1 || result.kind !== "exact-merge-qualification-result") {
    throw new Error("invalid qualification result discriminator");
  }
  if (result.result_sha256 !== qualificationResultHash(result)) {
    throw new Error("qualification result digest mismatch");
  }
  return result;
}

export function validateMergeReceipt(receipt) {
  const required = ["schema", "kind", "head_sha", "base_sha", "inputs", "lanes", "commands", "result_sha256", "authentication"];
  if (!exactKeys(receipt, required)) throw new Error("merge receipt has missing or unknown fields");
  if (receipt.schema !== 1 || receipt.kind !== "exact-merge-qualification") throw new Error("invalid merge receipt discriminator");
  if (!isSha1(receipt.head_sha) || !isSha1(receipt.base_sha) || receipt.head_sha === receipt.base_sha) {
    throw new Error("merge receipt does not bind distinct exact commits");
  }
  const inputKeys = [
    "rust_toolchain_sha256",
    "node_version_sha256",
    "cargo_lock_sha256",
    "vendor_inventory_sha256",
    "suite_inventory_sha256",
    "qualification_profile_sha256",
    "lane_selection_sha256",
  ];
  if (!exactKeys(receipt.inputs, inputKeys) || inputKeys.some((key) => !isSha256(receipt.inputs[key]))) {
    throw new Error("merge receipt input binding is incomplete");
  }
  if (!Array.isArray(receipt.lanes) || receipt.lanes.length === 0) throw new Error("merge receipt has no lanes");
  for (const lane of receipt.lanes) {
    if (!exactKeys(lane, ["name", "status", "result_sha256"]) || typeof lane.name !== "string" || lane.name.length === 0 || lane.status !== "success" || !isSha256(lane.result_sha256)) {
      throw new Error("merge receipt contains an invalid lane result");
    }
  }
  if (new Set(receipt.lanes.map((lane) => lane.name)).size !== receipt.lanes.length) throw new Error("merge receipt repeats a lane");
  if (!Array.isArray(receipt.commands) || receipt.commands.length === 0) throw new Error("merge receipt has no commands");
  for (const command of receipt.commands) {
    if (!exactKeys(command, ["argv", "exit_code", "stdout_sha256", "stderr_sha256"]) || !Array.isArray(command.argv) || command.argv.length === 0 || command.argv.some((arg) => typeof arg !== "string") || command.exit_code !== 0 || !isSha256(command.stdout_sha256) || !isSha256(command.stderr_sha256)) {
      throw new Error("merge receipt contains an invalid command result");
    }
  }
  if (!isSha256(receipt.result_sha256) || receipt.result_sha256 !== receiptResultHash(receipt)) {
    throw new Error("merge receipt result digest mismatch");
  }
  const authentication = receipt.authentication;
  if (!exactKeys(authentication, ["kind", "issuer", "subject", "attestation_sha256"]) || !["trusted-runner-oidc", "registered-signer"].includes(authentication.kind) || typeof authentication.issuer !== "string" || authentication.issuer.length === 0 || typeof authentication.subject !== "string" || authentication.subject.length === 0 || !isSha256(authentication.attestation_sha256)) {
    throw new Error("merge receipt lacks accepted authentication");
  }
  return receipt;
}

export function validateFailureArtifact(artifact, payload = undefined, limit = 10_485_760) {
  const required = ["schema", "kind", "head_sha", "base_sha", "track", "payload_path", "content_type", "bytes", "payload_sha256", "truncated"];
  if (!exactKeys(artifact, required, ["reproducer"])) throw new Error("failure artifact has missing or unknown fields");
  if (artifact.schema !== 1 || artifact.kind !== "failure-artifact" || !isSha1(artifact.head_sha) || !isSha1(artifact.base_sha)) {
    throw new Error("invalid failure artifact discriminator or commit binding");
  }
  if (!["common", "l0-l1", "h1", "host-platform", "stress", "performance"].includes(artifact.track)) throw new Error("unknown failure artifact track");
  if (!isRelativeRepositoryPath(artifact.payload_path) || typeof artifact.content_type !== "string" || artifact.content_type.length === 0 || artifact.content_type.length > 128) throw new Error("invalid failure artifact payload metadata");
  if (!Number.isInteger(artifact.bytes) || artifact.bytes < 0 || artifact.bytes > limit || !isSha256(artifact.payload_sha256) || typeof artifact.truncated !== "boolean") throw new Error("failure artifact exceeds its bound or has an invalid digest");
  if (payload !== undefined) {
    const bytes = Buffer.isBuffer(payload) ? payload : Buffer.from(payload);
    if (bytes.length !== artifact.bytes || sha256(bytes) !== artifact.payload_sha256) throw new Error("failure artifact payload binding mismatch");
  }
  if (artifact.reproducer !== undefined) {
    if (!exactKeys(artifact.reproducer, [], ["seed", "fixture", "initial_text_sha256", "options_key_sha256"])) throw new Error("failure artifact reproducer has unknown fields");
    if (artifact.reproducer.fixture !== undefined && !isRelativeRepositoryPath(artifact.reproducer.fixture)) throw new Error("failure artifact reproducer fixture is invalid");
    for (const key of ["initial_text_sha256", "options_key_sha256"]) {
      if (artifact.reproducer[key] !== undefined && !isSha256(artifact.reproducer[key])) throw new Error(`failure artifact ${key} is invalid`);
    }
  }
  return artifact;
}

export function loadPolicy() {
  return JSON.parse(fs.readFileSync(policyPath, "utf8"));
}

export function validatePolicy(policy) {
  if (!exactKeys(policy, ["schema", "status", "aggregate_check", "limits", "hosted_acceptance", "local_full_gate", "classification", "approved_performance"])) throw new Error("qualification policy has missing or unknown fields");
  if (policy.schema !== 2 || policy.status !== "active" || policy.aggregate_check !== "gates") throw new Error("invalid qualification policy header");
  if (policy.limits.changed_paths !== 4096 || policy.limits.failure_artifact_bytes !== 10_485_760) throw new Error("qualification bounds drifted");
  if (policy.classification.usage !== "local-evidence-tooling-only" || policy.classification.unknown_non_documentation !== "select-all") throw new Error("classification utility policy must be local-only and fail closed");

  const hosted = policy.hosted_acceptance;
  if (!exactKeys(hosted, ["authority_workflow", "authority_job", "test_root", "authoritative_command", "only_acceptance_tests", "rust_source_sha256"]) || hosted.authority_workflow !== ".github/workflows/ci.yml" || hosted.authority_job !== "gates" || hosted.test_root !== "ts-tests/" || canonical(hosted.authoritative_command) !== canonical(["cargo", "xtask", "acceptance"]) || hosted.only_acceptance_tests !== true) {
    throw new Error("invalid hosted ts-tests acceptance policy");
  }
  const local = policy.local_full_gate;
  if (!exactKeys(local, ["authoritative_command", "required_for_non_documentation", "documentation_only_exception"]) || canonical(local.authoritative_command) !== canonical(["cargo", "xtask", "ci", "--baseline", "<trusted-base>"]) || local.required_for_non_documentation !== true || local.documentation_only_exception !== true) {
    throw new Error("invalid local full-gate policy");
  }

  const workflow = fs.readFileSync(path.join(workspace, hosted.authority_workflow), "utf8");
  const jobsStart = workflow.indexOf("\njobs:\n");
  if (jobsStart < 0) throw new Error("hosted acceptance workflow has no jobs boundary");
  const jobIds = [...workflow.slice(jobsStart + 1).matchAll(/^  ([a-z][a-z0-9_-]*):\s*$/gmu)].map((match) => match[1]);
  if (canonical(jobIds) !== canonical([hosted.authority_job])) throw new Error("hosted acceptance workflow must contain only the gates job");
  const runCommands = [...workflow.matchAll(/^\s+run:\s+([^\n]+)$/gmu)].map((match) => match[1].trim());
  if (canonical(runCommands) !== canonical([hosted.authoritative_command.join(" ")])) throw new Error("hosted acceptance workflow must run only the pinned acceptance command");
  const actions = [...workflow.matchAll(/^\s+uses:\s+(\S+)/gmu)].map((match) => match[1]);
  if (canonical(actions) !== canonical(["actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"])) throw new Error("hosted acceptance workflow may use only the pinned checkout action");
  for (const forbidden of ["actions/setup-node", "runs-on: windows", "\n  schedule:"]) {
    if (workflow.includes(forbidden)) throw new Error(`hosted acceptance workflow contains forbidden non-acceptance work: ${forbidden}`);
  }
  const xtaskBytes = fs.readFileSync(path.join(workspace, "crates/xtask/src/main.rs"));
  const moduleSourceBytes = Object.fromEntries(
    HOSTED_ACCEPTANCE_MODULES.map((modulePath) => [
      modulePath,
      fs.readFileSync(path.join(workspace, modulePath)),
    ]),
  );
  const xtaskSource = xtaskBytes.toString("utf8");
  const moduleSources = Object.fromEntries(
    Object.entries(moduleSourceBytes).map(([modulePath, source]) => [
      modulePath,
      source.toString("utf8"),
    ]),
  );
  validateRustOwnerControlBoundaries({
    xtaskSource,
    moduleSources,
    sourceSha256: hosted.rust_source_sha256,
    rawSourceBytes: {
      "crates/xtask/src/main.rs": xtaskBytes,
      ...moduleSourceBytes,
    },
  });
  if (
    !exactKeys(policy.approved_performance, [
      "runner_profile",
      "authority_workflow",
      "authority_job",
      "environment",
      "evidence",
      "l1_authority_workflow",
      "l1_authority_job",
      "l1_h0_evidence",
      "l1_evidence",
      "h1_authority_workflow",
      "h1_authority_job",
      "h1_evidence",
      "moving_hosted_images_may_mint_ratchets",
      "alternating_baseline_candidate",
    ]) ||
    policy.approved_performance.authority_workflow !== ".github/workflows/l0-performance.yml" ||
    policy.approved_performance.authority_job !== "qualify" ||
    policy.approved_performance.environment !== "approved-performance" ||
    policy.approved_performance.evidence !==
      "ratchets/l0-one-shot-registry-performance.v1.json" ||
    policy.approved_performance.l1_authority_workflow !==
      ".github/workflows/l1-performance.yml" ||
    policy.approved_performance.l1_authority_job !== "qualify" ||
    policy.approved_performance.l1_h0_evidence !== "ratchets/l1-h0-performance.v1.json" ||
    policy.approved_performance.l1_evidence !==
      "ratchets/l1-incremental-parser-performance.v1.json" ||
    policy.approved_performance.h1_authority_workflow !==
      ".github/workflows/h1-noemit-performance.yml" ||
    policy.approved_performance.h1_authority_job !== "qualify" ||
    policy.approved_performance.h1_evidence !== "ratchets/h1-noemit-performance.v1.json"
  )
    throw new Error("invalid performance authority binding");
  if (!policy.approved_performance.alternating_baseline_candidate || policy.approved_performance.moving_hosted_images_may_mint_ratchets) throw new Error("invalid performance authority policy");
  for (const contract of [
    "lane-selection",
    "qualification-result",
    "merge-receipt",
    "failure-artifact",
    "acceptance-failure.v1",
    "h1-emit-profile",
    "h1-emit-observation",
    "h1-emit-performance",
    "h1-emit-qualification",
    "h1-owner-inventory",
    "h1-noemit-performance",
    "h1-rust-omissions",
    "h1-printer-foundation",
    "h2-owner-inventory",
    "h2-candidate-dispositions",
    "h2-profile-transition",
    "h2-runtime-baseline",
    "h2-1a-qualification",
    "h2-1a-profile",
    "h2-1b-qualification",
    "h2-1b-profile",
    "h2-1c-owner-controls",
    "h2-1c-qualification",
    "h2-1c-profile",
    "h2-1d-owner-controls",
    "h2-1d-qualification",
    "h2-1d-profile",
    "h2-1e-owner-controls",
    "h2-1e-qualification",
    "h2-1e-profile",
    "h2-2a-qualification",
    "h2-2a-profile",
    "h2-2b-qualification",
    "h2-2b-profile",
    "h2-2c-qualification",
    "h2-2c-profile",
    "h2-2d-qualification",
    "h2-2d-profile",
    "h2-3a-owner-controls",
    "h2-3a-qualification",
    "h2-3a-profile",
    "h2-3b-owner-controls",
    "h2-3b-qualification",
    "h2-3b-profile",
    "h2-3c-owner-controls",
    "h2-3c-qualification",
    "h2-3c-profile",
    "h2-3d-owner-controls",
    "h2-3d-qualification",
    "h2-3d-profile",
    "h2-4a-owner-controls",
    "h2-4a-qualification",
    "h2-4a-profile",
    "h2-4b-owner-controls",
    "h2-4b-qualification",
    "h2-4b-profile",
    "h2-5a-owner-controls",
    "h2-5a-qualification",
    "h2-5a-profile",
    "h2-5b-owner-controls",
    "h2-5b-qualification",
    "h2-5b-profile",
    "h2-5c-owner-controls",
    "h2-5c-qualification",
    "h2-5c-profile",
    "h2-5d-owner-controls",
    "h2-5d-qualification",
    "h2-5d-profile",
    "h2-5e-owner-controls",
    "h2-5e-qualification",
    "h2-5e-profile",
    "h2-5f-owner-controls",
    "h2-5f-qualification",
    "h2-5f-profile",
    "h2-5g-owner-controls",
    "h2-5g-qualification",
    "h2-5g-profile",
    "h2-5h-qualification",
    "h2-6a-qualification",
    "h2-source-reachability",
    "h2-emit-observation",
  ]) {
    const schema = JSON.parse(fs.readFileSync(path.join(contractDirectory, `${contract}.schema.json`), "utf8"));
    if (schema.$schema !== "https://json-schema.org/draft/2020-12/schema" || schema.additionalProperties !== false || !schema.$id.endsWith(`/${contract}.schema.json`)) {
      throw new Error(`invalid ${contract} JSON schema boundary`);
    }
  }
  return policy;
}

function fileSha256(filePath) {
  return sha256(fs.readFileSync(path.join(workspace, filePath)));
}

function trackedTreeDigest(prefixes, exact = []) {
  const exactSet = new Set(exact);
  const paths = execFileSync("git", ["ls-files", "-z"], {
    cwd: workspace,
    maxBuffer: 64 * 1024 * 1024,
  })
    .toString("utf8")
    .split("\0")
    .filter(Boolean)
    .filter((entry) => exactSet.has(entry) || prefixes.some((prefix) => entry.startsWith(prefix)))
    .sort((left, right) => left.localeCompare(right));
  const hash = crypto.createHash("sha256");
  for (const entry of paths) {
    hash.update(entry);
    hash.update("\0");
    hash.update(fileSha256(entry));
    hash.update("\0");
  }
  return hash.digest("hex");
}

function qualificationInputs(selection) {
  return {
    rust_toolchain_sha256: fileSha256("rust-toolchain.toml"),
    node_version_sha256: fileSha256(".node-version"),
    cargo_lock_sha256: fileSha256("Cargo.lock"),
    vendor_inventory_sha256: trackedTreeDigest(["vendor/"]),
    suite_inventory_sha256: trackedTreeDigest([
      "baselines/",
      "tests/",
      "ratchets/",
      "crates/oracle/",
      "crates/conformance/tests/",
      "crates/harness/tests/",
    ]),
    qualification_profile_sha256: trackedTreeDigest(
      [".github/ci/"],
      [
        ".github/workflows/ci.yml",
        ".github/workflows/l0-performance.yml",
        ".github/workflows/l1-performance.yml",
      ],
    ),
    lane_selection_sha256: sha256(canonical(selection)),
  };
}

function produceQualificationResult({ baseSha, headSha, selectionPath, stdoutPath, stderrPath }) {
  const exactBase = git("rev-parse", "--verify", `${baseSha}^{commit}`);
  const exactHead = git("rev-parse", "--verify", `${headSha}^{commit}`);
  if (git("rev-parse", "HEAD") !== exactHead || exactBase === exactHead) {
    throw new Error("qualification result is not running at the declared distinct HEAD/base");
  }
  if (git("status", "--porcelain").length !== 0) {
    throw new Error("qualification result refuses a dirty candidate worktree");
  }
  const selection = validateLaneSelection(JSON.parse(fs.readFileSync(selectionPath, "utf8")), loadPolicy().limits.changed_paths);
  if (selection.head_sha !== exactHead || selection.base_sha !== exactBase || selection.docs_only) {
    throw new Error("qualification selection does not bind this executable HEAD/base");
  }
  const command = {
    argv: ["cargo", "xtask", "ci", "--baseline", exactBase],
    exit_code: 0,
    stdout_sha256: sha256(fs.readFileSync(stdoutPath)),
    stderr_sha256: sha256(fs.readFileSync(stderrPath)),
  };
  const result = {
    schema: 1,
    kind: "exact-merge-qualification-result",
    head_sha: exactHead,
    base_sha: exactBase,
    inputs: qualificationInputs(selection),
    lanes: [{ name: "full", status: "success", result_sha256: sha256(canonical(command)) }],
    commands: [command],
    result_sha256: "0".repeat(64),
  };
  result.result_sha256 = qualificationResultHash(result);
  return validateQualificationResult(result);
}

function finalizeReceipt(result, bundle, issuer, subject) {
  validateQualificationResult(result);
  if (!issuer || !subject) throw new Error("receipt finalization requires an OIDC issuer and subject");
  JSON.parse(bundle.toString("utf8"));
  const receipt = {
    ...result,
    kind: "exact-merge-qualification",
    authentication: {
      kind: "trusted-runner-oidc",
      issuer,
      subject,
      attestation_sha256: sha256(bundle),
    },
  };
  receipt.result_sha256 = receiptResultHash(receipt);
  return validateMergeReceipt(receipt);
}

export function validateBoundReceipt(receipt, result, bundle, headSha, baseSha) {
  validateMergeReceipt(receipt);
  validateQualificationResult(result);
  if (receipt.head_sha !== headSha || receipt.base_sha !== baseSha || result.head_sha !== headSha || result.base_sha !== baseSha) {
    throw new Error("authenticated receipt does not bind the expected HEAD/base");
  }
  for (const key of ["inputs", "lanes", "commands"]) {
    if (canonical(receipt[key]) !== canonical(result[key])) {
      throw new Error(`authenticated receipt ${key} differ from the attested result`);
    }
  }
  if (receipt.authentication.attestation_sha256 !== sha256(bundle)) {
    throw new Error("authenticated receipt does not bind the verified attestation bundle");
  }
  return receipt;
}

function writeJson(target, value) {
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, `${JSON.stringify(value, null, 2)}\n`);
}

function produceFailureArtifact({ headSha, baseSha, track, sourcePath, payloadPath, contentType, seed, fixture, initialTextSha256 }) {
  const policy = validatePolicy(loadPolicy());
  const exactHead = git("rev-parse", "--verify", `${headSha}^{commit}`);
  const exactBase = git("rev-parse", "--verify", `${baseSha}^{commit}`);
  if (exactHead === exactBase || !isRelativeRepositoryPath(payloadPath)) {
    throw new Error("failure artifact requires distinct commits and a repository-relative payload path");
  }
  const source = fs.readFileSync(sourcePath);
  const limit = policy.limits.failure_artifact_bytes;
  let payload = source;
  let truncated = false;
  if (source.length > limit) {
    const marker = Buffer.from("\n...[failure payload truncated to policy bound]...\n", "utf8");
    const side = Math.floor((limit - marker.length) / 2);
    payload = Buffer.concat([source.subarray(0, side), marker, source.subarray(source.length - side)]);
    truncated = true;
  }
  const absolutePayload = path.resolve(workspace, payloadPath);
  if (absolutePayload !== workspace && !absolutePayload.startsWith(`${workspace}${path.sep}`)) {
    throw new Error("failure payload escapes the repository workspace");
  }
  fs.mkdirSync(path.dirname(absolutePayload), { recursive: true });
  fs.writeFileSync(absolutePayload, payload);
  const reproducer = {};
  if (seed) reproducer.seed = seed;
  if (fixture) reproducer.fixture = fixture;
  if (initialTextSha256) reproducer.initial_text_sha256 = initialTextSha256;
  const artifact = {
    schema: 1,
    kind: "failure-artifact",
    head_sha: exactHead,
    base_sha: exactBase,
    track,
    payload_path: payloadPath,
    content_type: contentType,
    bytes: payload.length,
    payload_sha256: sha256(payload),
    truncated,
    ...(Object.keys(reproducer).length > 0 ? { reproducer } : {}),
  };
  return validateFailureArtifact(artifact, payload, limit);
}

function git(...args) {
  return execFileSync("git", args, { cwd: workspace, encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).trim();
}

function statusBlock(commit) {
  const readme = execFileSync("git", ["show", `${commit}:README.md`], { cwd: workspace, encoding: "utf8" });
  const begins = [...readme.matchAll(/<!-- STATUS:BEGIN /gu)];
  const ends = [...readme.matchAll(/<!-- STATUS:END -->/gu)];
  if (begins.length !== 1 || ends.length !== 1 || begins[0].index >= ends[0].index) throw new Error("invalid README status block");
  return readme.slice(begins[0].index, ends[0].index + ends[0][0].length);
}

function changedPaths(baseSha, headSha) {
  const output = execFileSync("git", ["diff", "--name-only", "--no-renames", "-z", baseSha, headSha], {
    cwd: workspace,
    maxBuffer: 16 * 1024 * 1024,
  });
  return output.toString("utf8").split("\0").filter(Boolean);
}

function argument(name) {
  const index = process.argv.indexOf(name);
  return index >= 0 ? process.argv[index + 1] : undefined;
}

function writeGithubOutput(selection, target) {
  const tracks = new Set(selection.selected.tracks);
  const lines = [
    `head_sha=${selection.head_sha}`,
    `base_sha=${selection.base_sha}`,
    `docs_only=${selection.docs_only}`,
    `static=${selection.selected.static}`,
    `host_platform=${selection.selected.host_platform}`,
    `program_path=${selection.selected.program_path}`,
    `l0_l1=${tracks.has("l0-l1")}`,
    `h1=${tracks.has("h1")}`,
    `selection_sha256=${sha256(canonical(selection))}`,
  ];
  fs.appendFileSync(target, `${lines.join("\n")}\n`);
}

function main() {
  const command = process.argv[2];
  if (command === "check") {
    validatePolicy(loadPolicy());
    const contracts = validateArtifactSchemaContracts();
    const readiness = validateFciReadinessChain();
    process.stdout.write(
      `CI qualification policy, schemas, ${contracts.length} registered artifact contracts, and the FCI readiness chain (${readiness.envelopes} envelopes, ${readiness.ready} ready) are valid\n`,
    );
    return;
  }
  if (command === "verify-selection") {
    const input = argument("--path");
    if (!input) throw new Error("verify-selection requires --path");
    validateLaneSelection(JSON.parse(fs.readFileSync(input, "utf8")), loadPolicy().limits.changed_paths);
    process.stdout.write("lane selection is valid\n");
    return;
  }
  if (command === "verify-receipt") {
    const input = argument("--path");
    if (!input) throw new Error("verify-receipt requires --path");
    validateMergeReceipt(JSON.parse(fs.readFileSync(input, "utf8")));
    process.stdout.write("authenticated exact merge receipt is valid\n");
    return;
  }
  if (command === "produce-result") {
    const baseSha = argument("--base");
    const headSha = argument("--head");
    const selectionPath = argument("--selection");
    const stdoutPath = argument("--stdout");
    const stderrPath = argument("--stderr");
    const output = argument("--out");
    if (!baseSha || !headSha || !selectionPath || !stdoutPath || !stderrPath || !output) {
      throw new Error("produce-result requires --base, --head, --selection, --stdout, --stderr, and --out");
    }
    const result = produceQualificationResult({ baseSha, headSha, selectionPath, stdoutPath, stderrPath });
    writeJson(path.resolve(output), result);
    process.stdout.write(`wrote exact qualification result ${output}\n`);
    return;
  }
  if (command === "finalize-receipt") {
    const resultPath = argument("--result");
    const bundlePath = argument("--bundle");
    const issuer = argument("--issuer");
    const subject = argument("--subject");
    const output = argument("--out");
    if (!resultPath || !bundlePath || !issuer || !subject || !output) {
      throw new Error("finalize-receipt requires --result, --bundle, --issuer, --subject, and --out");
    }
    const receipt = finalizeReceipt(
      JSON.parse(fs.readFileSync(resultPath, "utf8")),
      fs.readFileSync(bundlePath),
      issuer,
      subject,
    );
    writeJson(path.resolve(output), receipt);
    process.stdout.write(`wrote authenticated exact merge receipt ${output}\n`);
    return;
  }
  if (command === "verify-bound-receipt") {
    const receiptPath = argument("--receipt");
    const resultPath = argument("--result");
    const bundlePath = argument("--bundle");
    const headSha = argument("--head");
    const baseSha = argument("--base");
    if (!receiptPath || !resultPath || !bundlePath || !headSha || !baseSha) {
      throw new Error("verify-bound-receipt requires --receipt, --result, --bundle, --head, and --base");
    }
    validateBoundReceipt(
      JSON.parse(fs.readFileSync(receiptPath, "utf8")),
      JSON.parse(fs.readFileSync(resultPath, "utf8")),
      fs.readFileSync(bundlePath),
      headSha,
      baseSha,
    );
    process.stdout.write("authenticated receipt is bound to the verified result and exact HEAD/base\n");
    return;
  }
  if (command === "verify-failure") {
    const input = argument("--path");
    const payloadPath = argument("--payload");
    if (!input || !payloadPath) throw new Error("verify-failure requires --path and --payload");
    validateFailureArtifact(
      JSON.parse(fs.readFileSync(input, "utf8")),
      fs.readFileSync(payloadPath),
      loadPolicy().limits.failure_artifact_bytes,
    );
    process.stdout.write("bounded failure artifact is valid\n");
    return;
  }
  if (command === "write-failure") {
    const headSha = argument("--head");
    const baseSha = argument("--base");
    const track = argument("--track");
    const sourcePath = argument("--source");
    const payloadPath = argument("--payload-path");
    const contentType = argument("--content-type") ?? "text/plain";
    const output = argument("--out");
    if (!headSha || !baseSha || !track || !sourcePath || !payloadPath || !output) {
      throw new Error("write-failure requires --head, --base, --track, --source, --payload-path, and --out");
    }
    const artifact = produceFailureArtifact({
      headSha,
      baseSha,
      track,
      sourcePath,
      payloadPath,
      contentType,
      seed: argument("--seed"),
      fixture: argument("--fixture"),
      initialTextSha256: argument("--initial-text-sha256"),
    });
    writeJson(path.resolve(output), artifact);
    process.stdout.write(`wrote bounded failure artifact ${output}\n`);
    return;
  }
  if (command === "classify") {
    const policy = validatePolicy(loadPolicy());
    const baseRef = argument("--base");
    const headRef = argument("--head");
    if (!baseRef || !headRef) throw new Error("classify requires --base and --head");
    const baseSha = git("rev-parse", "--verify", `${baseRef}^{commit}`);
    const headSha = git("rev-parse", "--verify", `${headRef}^{commit}`);
    let equal = false;
    try {
      equal = statusBlock(baseSha) === statusBlock(headSha);
    } catch {
      equal = false;
    }
    const selection = classifyPaths({
      paths: changedPaths(baseSha, headSha),
      headSha,
      baseSha,
      statusBlockEqual: equal,
      policy,
    });
    const rendered = `${JSON.stringify(selection, null, 2)}\n`;
    const output = argument("--out");
    const githubOutput = argument("--github-output");
    if (output) fs.writeFileSync(output, rendered);
    if (githubOutput) writeGithubOutput(selection, githubOutput);
    process.stdout.write(rendered);
    return;
  }
  throw new Error("usage: qualification.mjs check|classify|produce-result|finalize-receipt|verify-bound-receipt|write-failure|verify-selection|verify-receipt|verify-failure ...");
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.stack ?? error}\n`);
    process.exitCode = 1;
  }
}
