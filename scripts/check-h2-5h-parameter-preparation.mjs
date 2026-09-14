// Checks preparation inputs, or the Codex lane's changed paths after editing.
// Neither mode certifies runtime readiness or audits Claude's working diff.
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = relative => fs.readFileSync(path.join(root, relative));
const sha = bytes => crypto.createHash('sha256').update(bytes).digest('hex');
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' });
const packet = JSON.parse(read('docs/design/greenfield/slices/h2-5h-parameter-temporaries-selection.v1.json'));
assert.equal(packet.schema, 'h2-5h-parameter-temporaries-preparation/v1');
assert.equal(packet.status, 'selection-and-source-inventory-not-runtime-ready');
assert.ok(process.argv.length === 2 || (process.argv.length === 3 && process.argv[2] === '--scope'),
  'usage: node scripts/check-h2-5h-parameter-preparation.mjs [--scope]');
git('merge-base', '--is-ancestor', packet.base_head, 'HEAD');
const { codex, claude, shared_forbidden_prefixes: shared } = packet.ownership;
const allowed = [...codex.existing_production, ...codex.planned_new, ...codex.preparation];
assert.equal(new Set(allowed).size, allowed.length, 'duplicate allowed path');
for (const relative of allowed) {
  assert.ok(!path.isAbsolute(relative) && !relative.split('/').includes('..'), `unsafe path: ${relative}`);
  assert.ok(![...claude.reserved_prefixes, ...shared].some(prefix => relative.startsWith(prefix)),
    `reserved/shared path in Codex ticket: ${relative}`);
  assert.ok(!claude.initial_and_conditional_production.includes(relative), `production overlap: ${relative}`);
  assert.ok(!/(?:^|\/)contracts\.rs$/.test(relative), `shared registration: ${relative}`);
}
if (process.argv[2] === '--scope') {
  const tracked = git('diff', '--name-only', '-z', packet.base_head, '--').split('\0').filter(Boolean);
  const untracked = git('ls-files', '--others', '--exclude-standard', '-z').split('\0').filter(Boolean);
  const changed = [...new Set([...tracked, ...untracked])];
  for (const relative of changed) assert.ok(allowed.includes(relative), `outside Codex ticket: ${relative}`);
  console.log(`Codex scope verified: ${changed.length} changed paths allowed; ticket production overlap 0. Claude's actual diff and runtime behavior were not checked.`);
} else {
  for (const [relative, expected] of Object.entries(packet.files)) {
    assert.equal(sha(read(relative)), expected, `stale preparation input: ${relative}`);
  }
  for (const relative of codex.existing_production) assert.ok(Object.hasOwn(packet.files, relative));
  const require = createRequire(import.meta.url);
  const ts = require(path.join(root, 'vendor/typescript-6.0.3/lib/typescript.js'));
  assert.equal(ts.version, packet.typescript.version);
  const upstream = read(packet.typescript.path);
  assert.equal(sha(upstream), packet.typescript.sha256);
  const source = ts.createSourceFile(packet.typescript.path, upstream.toString('utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const ids = new Set(packet.typescript.owners.map(owner => owner.id));
  const owners = [];
  function visit(node, ancestors = []) {
    const name = ts.isFunctionDeclaration(node) && node.name ? node.name.text : null;
    const id = [...ancestors, name].filter(Boolean).join('/');
    if (name && ids.has(id)) {
      const start = node.getStart(source);
      owners.push({
        id, name,
        start_line: source.getLineAndCharacterOfPosition(start).line + 1,
        end_line: source.getLineAndCharacterOfPosition(node.end).line + 1,
        sha256_utf8_no_leading_trivia: sha(source.text.slice(start, node.end)),
      });
    }
    ts.forEachChild(node, child => visit(child, name ? [...ancestors, name] : ancestors));
  }
  visit(source);
  assert.deepEqual(owners, packet.typescript.owners);
  const manifest = JSON.parse(read(packet.manifest.path));
  const selectedIds = new Set(packet.manifest.selected_rows.map(row => row.case_id));
  assert.equal(manifest.cases.length, 16);
  assert.equal(selectedIds.size, 4);
  assert.deepEqual(manifest.cases.filter(row => selectedIds.has(row.case_id)), packet.manifest.selected_rows);
  assert.deepEqual(manifest.cases.filter(row => !selectedIds.has(row.case_id)), packet.manifest.preserved_rows);
  const artifacts = new Map();
  assert.equal(packet.cases.length, 8);
  assert.equal(new Set(packet.cases.map(row => row.case_id)).size, 8);
  for (const pinned of packet.cases) {
    if (!artifacts.has(pinned.artifact)) artifacts.set(pinned.artifact, JSON.parse(read(pinned.artifact)));
    const matches = artifacts.get(pinned.artifact).cases.filter(row => row.case_id === pinned.case_id);
    assert.equal(matches.length, 1, `case identity: ${pinned.case_id}`);
    const row = matches[0];
    assert.equal(sha(JSON.stringify(row)), pinned.sha256_json_stringify);
    assert.equal(sha(JSON.stringify(row.input)), pinned.input_sha256_json_stringify);
    assert.equal(sha(JSON.stringify(row.typescript_observation)), pinned.typescript_observation_sha256_json_stringify);
    assert.equal(sha(read(pinned.source_path)), pinned.source_sha256);
    assert.equal(row.execution_route, pinned.execution_route);
    assert.equal(row.disposition, 'admitted-for-execution');
    assert.deepEqual(row.typescript_run_fingerprints, pinned.typescript_run_fingerprints);
    assert.deepEqual(row.typescript_run_fingerprints, Array(2).fill(row.typescript_observation.run_fingerprint_sha256));
    assert.equal(row.typescript_observation.exit_code, pinned.command_exit_code);
    assert.deepEqual(row.typescript_observation.reported_diagnostics.map(diagnostic => diagnostic.code), pinned.reported_diagnostic_codes);
  }
  assert.equal(packet.new_native_baseline.status, 'not-run');
  assert.equal(packet.implementation_design.status, 'pending');
  console.log(`Parameter preparation verified: ${Object.keys(packet.files).length} input pins, ${owners.length} upstream owners, 4 ES5 + 4 ES2015 frozen cases, 12 preserved manifest rows, ticket production overlap 0. Fresh native baseline and runtime readiness remain pending.`);
}
