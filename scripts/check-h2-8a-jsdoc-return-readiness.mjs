// Readiness check for the H2.8a G5c JSDoc return slice: pinned upstream owner
// spans (whole lines, the ledger algorithm), the frozen control fixture and
// its counts, the Rust symbols the design names, and the design anchors.
// Fail-closed: a missing pin, symbol or count is a failure, never filled in.
import assert from "node:assert/strict";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = relative => fs.readFileSync(path.join(root, relative));
const sha = value => crypto.createHash("sha256").update(value).digest("hex");
const manifest = JSON.parse(read("docs/design/greenfield/slices/h2-8a-jsdoc-return-readiness.v1.json"));
assert.equal(manifest.schema, 1);
const upstream = read(manifest.typescript.path);
assert.equal(sha(upstream), manifest.typescript.sha256);
const lines = upstream.toString("latin1").split("\n");
for (const owner of manifest.typescript.owners) {
  const span = Buffer.from(lines.slice(owner.start_line - 1, owner.end_line).join("\n") + "\n", "latin1");
  assert.equal(sha(span), owner.sha256_whole_lines, `upstream owner drifted: ${owner.name}`);
  assert.ok(lines[owner.start_line - 1].includes(owner.first_line_contains), `owner start mismatch: ${owner.name}`);
}
const fixture = JSON.parse(read(manifest.fixture.path));
assert.equal(sha(read(manifest.fixture.path)), manifest.fixture.sha256);
assert.equal(fixture.cases.length, manifest.fixture.cases);
assert.equal(fixture.repetitions, 2);
assert.equal(fixture.cases.reduce((n, c) => n + c.return_trace.length, 0), manifest.fixture.traced_functions);
assert.deepEqual([...new Set(fixture.cases.map(c => c.group))].sort(), manifest.fixture.groups);
for (const [relative, symbols] of Object.entries(manifest.rust_symbols)) {
  const text = read(relative).toString("utf8");
  for (const symbol of symbols.present) assert.ok(text.includes(symbol), `${relative}: missing ${symbol}`);
  for (const symbol of symbols.absent ?? []) assert.ok(!text.includes(symbol), `${relative}: retired symbol still present ${symbol}`);
}
const design = read(manifest.design.path).toString("utf8");
for (const anchor of manifest.design.anchors) assert.ok(design.includes(anchor), `design anchor missing: ${anchor}`);
console.log(`G5c readiness verified: ${manifest.typescript.owners.length} upstream owners, fixture ${manifest.fixture.cases} cases / ${manifest.fixture.traced_functions} traced functions, ${Object.keys(manifest.rust_symbols).length} Rust files, ${manifest.design.anchors.length} design anchors (state: ${manifest.state}).`);
