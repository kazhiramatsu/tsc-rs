#!/usr/bin/env python3
"""Check frozen design assumptions; this never authorizes runtime activation."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / 'docs/design/greenfield/slices/h2-8b-e-design-registry.v1.json'


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--registry', type=Path, default=DEFAULT)
    args = parser.parse_args()
    registry = json.loads(args.registry.read_text())
    require(registry['version'] == 1, 'unsupported registry version')
    base = registry['base']
    tree = subprocess.check_output(
        ['git', 'rev-parse', base['head'] + '^{tree}'], cwd=ROOT, text=True).strip()
    require(tree == base['tree'], 'base tree mismatch')
    require(subprocess.run(['git', 'merge-base', '--is-ancestor', base['head'], 'HEAD'],
                           cwd=ROOT, check=False).returncode == 0, 'base is not an ancestor')
    pins = {p['path']: p['sha256'] for p in registry['pinned_files']}
    require(len(pins) == len(registry['pinned_files']), 'duplicate pinned file')
    for path, expected in pins.items():
        require(digest((ROOT / path).read_bytes()) == expected, 'stale file: ' + path)
    anchors = {a['id']: a for a in registry['source_anchors']}
    require(len(anchors) == len(registry['source_anchors']), 'duplicate source anchor')
    for name, anchor in anchors.items():
        require(anchor['path'] in pins, 'unbound source anchor: ' + name)
        lines = (ROOT / anchor['path']).read_bytes().splitlines(keepends=True)
        start, end = anchor['start_line'], anchor['end_line']
        require(1 <= start <= end <= len(lines), 'bad source range: ' + name)
        require(digest(b''.join(lines[start - 1:end])) == anchor['sha256'],
                'stale source anchor: ' + name)
        require(name.encode() in b''.join(lines[start - 1:end]), 'missing source symbol: ' + name)
    slices = {s['id']: s for s in registry['slices']}
    require(len(slices) == len(registry['slices']) == 24, 'slice count or duplicate ID')
    externals = registry['external_dependencies']
    visited, visiting = set(), set()

    def visit(name):
        require(name in slices or name in externals, 'unknown dependency: ' + name)
        if name in externals or name in visited:
            return
        require(name not in visiting, 'dependency cycle: ' + name)
        visiting.add(name)
        for dependency in slices[name]['depends_on']:
            visit(dependency)
        visiting.remove(name)
        visited.add(name)

    for name, row in slices.items():
        visit(name)
        require(row['production_edits_authorized'] is False, 'runtime authority in design registry')
        require(row['state'] in ('outline', 'ready-for-baseline'), 'unsupported readiness promotion')
        require(row['writer'] and row['witness_axes'] and row['source_anchor_ids'], 'incomplete outline: ' + name)
        require(all(p in pins for p in row['owner_files']), 'unbound Rust owner: ' + name)
        require(all(a in anchors for a in row['source_anchor_ids']), 'unknown source owner: ' + name)
        if row['state'] == 'outline':
            require(bool(row['unresolved']), 'outline must state unresolved work: ' + name)
        else:
            require(row['kind'] == 'evidence-only' and not row['unresolved'], 'baseline scope is not ready')
            require((ROOT / row['packet']).is_file(), 'missing executable evidence packet')
    first = registry['first_packet']
    ready = [s['id'] for s in slices.values() if s['state'] == 'ready-for-baseline']
    require(ready == [first['id']], 'unexpected ready-for-baseline set')
    require(first['native_executions'] == slices[first['id']]['native_executions'] == 0,
            'this preparation record cannot claim native execution')
    artifact = json.loads((ROOT / first['observations']).read_text())
    inputs = json.loads((ROOT / first['inputs']).read_text())
    case_ids = [c['case_id'] for c in inputs['cases']]
    require(len(case_ids) == len(set(case_ids)) == 12, 'witness membership/count mismatch')
    require(case_ids == first['case_ids'] == [c['case_id'] for c in artifact['cases']],
            'witness membership/order mismatch')
    require(artifact['typescript'] == base['typescript'] == '6.0.3', 'TypeScript version mismatch')
    require(artifact['source_commit'] == base['typescript_source_commit'], 'TypeScript source mismatch')
    require(artifact['compiler_sha256'] == pins['vendor/typescript-6.0.3/lib/typescript.js'], 'compiler binding')
    require(artifact['observer_sha256'] == pins[first['observer']], 'observer binding')
    require(artifact['inputs'] == {'path': first['inputs'], 'sha256': pins[first['inputs']]}, 'input binding')
    require(artifact['repetitions'] == first['repetitions'] == 2, 'repetition count')
    require(artifact['program_attempts'] == artifact['program_executions']
            == first['upstream_program_executions'] == 24, 'upstream execution count')
    require(artifact['exception_attempts'] == 0 and artifact['upstream_failures'] == [], 'upstream failure')
    for source, case in zip(inputs['cases'], artifact['cases']):
        require({k: v for k, v in case.items() if k != 'typescript_observation'} == source,
                'observation input drift: ' + case['case_id'])
        observation = case['typescript_observation']
        require(set(observation['program_facts']) == {'source_files', 'library_files', 'root_names'},
                'missing Program membership facts')
        require(all(isinstance(v, list) and all(isinstance(p, str) for p in v)
                    for v in observation['program_facts'].values()), 'malformed membership facts')
        require(all(key in observation for key in ('writes', 'reported_diagnostics', 'emit_result',
                                                   'status_writes', 'exit_code')), 'incomplete command')
    print(json.dumps({'slices': len(slices), 'outline': 23, 'ready_for_baseline': ready,
                      'runtime_ready': 0, 'pinned_files': len(pins), 'source_anchors': len(anchors),
                      'upstream_cases': 12, 'upstream_program_executions': 24,
                      'native_executions': 0, 'status': 'pass'}))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, OSError, subprocess.CalledProcessError) as error:
        print('FAIL: ' + str(error), file=sys.stderr)
        sys.exit(1)
