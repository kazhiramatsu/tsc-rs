#!/usr/bin/env python3
"""Validate the CFG completion supplement without rewriting its frozen design registry."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--receipt', type=Path, default=ROOT / 'ratchets/h2-8b-config-completion-final.v1.json')
    parser.add_argument('--local-captures', action='store_true')
    args = parser.parse_args()
    receipt = json.loads(args.receipt.read_text())
    require(receipt['version'] == 1 and receipt['slice'] == 'H2.8b-CFG1', 'receipt identity')
    require(receipt['status'] == 'implemented-and-locally-verified', 'completion state')
    require(not receipt['profile_activation'] and not receipt['hosted_acceptance_claim'], 'unexpected qualification claim')
    subprocess.run(['git', 'merge-base', '--is-ancestor', receipt['measured_head'], 'HEAD'], cwd=ROOT, check=True)
    for group in ['production_sha256', 'artifacts_sha256', 'preserved_sha256']:
        require(bool(receipt[group]), 'empty binding: ' + group)
        for path, expected in receipt[group].items():
            require(sha(ROOT / path) == expected, 'stale file: ' + path)
    history = receipt['historical_design']
    require(history['exit_code'] == 0, 'historical design audit failed')
    registry = json.loads((ROOT / history['registry']).read_text())
    require(sha(ROOT / history['registry']) == history['registry_sha256'], 'historical registry drift')
    cfg = next(row for row in registry['slices'] if row['id'] == receipt['slice'])
    require(set(receipt['resolved_witness_axes']) == set(cfg['witness_axes']), 'incomplete original witness axes')
    require(set(receipt['resolved_design_questions']) == set(cfg['unresolved']), 'unresolved original design questions')
    require(all(receipt['resolved_witness_axes'].values()) and all(receipt['resolved_design_questions'].values()), 'empty resolution')
    require(not receipt['unresolved_cfg_work'], 'CFG work remains')
    for anchor in receipt['source_anchors']:
        data = (ROOT / anchor['path']).read_bytes().splitlines(keepends=True)
        actual = hashlib.sha256(b''.join(data[anchor['start_line'] - 1:anchor['end_line']])).hexdigest()
        require(actual == anchor['sha256'], 'stale source anchor: ' + anchor['symbol'])
        require(anchor['symbol'].encode() in b''.join(data[anchor['start_line'] - 1:anchor['end_line']]), 'missing source symbol: ' + anchor['symbol'])
    for expected, manifest in receipt['input_manifests'].items():
        actual = hashlib.sha256(json.dumps(manifest, sort_keys=True).encode()).hexdigest()
        require(actual == expected, 'run input manifest digest')
    runs = {run['phase']: run for run in receipt['runs']}
    require(len(runs) == len(receipt['runs']), 'duplicate run phase')
    for phase in receipt['required_successful_phases']:
        run = runs[phase]
        require(run['exit_code'] == 0 and run['inputs_unchanged'], 'failed/dirty final run: ' + phase)
        require(run['head'] == receipt['measured_head'], 'final run head mismatch: ' + phase)
        require(bool(run['executables']), 'missing measured executable: ' + phase)
        manifest = receipt['input_manifests'][run['input_manifest_sha256']]
        for path, expected in receipt['production_sha256'].items():
            require(manifest[path] == expected, 'production differs from measured input: ' + path)
    cli = runs['config-final-cli']
    require(cli['cli_binary_unchanged'] and cli['cli_binary_before']['sha256'] == cli['cli_binary_after']['sha256'], 'CLI product binary changed during measurement')
    clippy = runs['config-final-clippy']
    require(clippy['exit_code'] == 0 and clippy['inputs_unchanged'] and clippy['head'] == receipt['measured_head'], 'final clippy result')
    projections = commands = boundaries = 0
    for group in receipt['observation_groups']:
        artifact = json.loads((ROOT / group['path']).read_text())
        cases = artifact['cases']
        require(len(cases) == group['cases'], 'observation count: ' + group['path'])
        identifiers = [case['case_id'] for case in cases]
        require(len(set(identifiers)) == len(identifiers), 'duplicate case ID: ' + group['path'])
        require(artifact['repetitions'] == 2, 'repetition count: ' + group['path'])
        if group['kind'] == 'config-projection':
            projections += len(cases)
        elif group['kind'] == 'complete-command':
            excluded = group.get('boundary_case_ids', [])
            require(set(excluded).issubset(identifiers), 'unknown boundary case')
            boundaries += len(excluded)
            commands += len(cases) - len(excluded)
    require(projections == receipt['counts']['config_projection_cases'], 'projection total does not match artifacts')
    require(commands == receipt['counts']['complete_commands'], 'command total does not match artifacts')
    require(boundaries == receipt['counts']['module_boundary_controls'], 'boundary total does not match artifacts')
    require((commands + boundaries) * 4 == receipt['counts']['prepared_programs'], 'Program total does not match artifacts')
    require(receipt['counts']['complete_commands'] == 86, 'complete command membership')
    require(receipt['counts']['module_boundary_controls'] == 2, 'module boundary membership')
    require(receipt['counts']['prepared_programs'] == 352, 'Program attempt count')
    require(receipt['counts']['config_projection_cases'] == 236, 'config projection membership')
    require(receipt['counts']['cache_host_cases'] == 26 and receipt['counts']['cache_host_parse_attempts'] == 60, 'cache/host membership')
    if args.local_captures:
        for capture in receipt['captures']:
            require(sha(Path(capture['path'])) == capture['sha256'], 'stale capture: ' + capture['path'])
        for run in receipt['runs']:
            require(sha(Path(run['log']['path'])) == run['log']['sha256'], 'stale run log: ' + run['phase'])
    print(json.dumps({'slice': receipt['slice'], 'status': 'pass', 'measured_head': receipt['measured_head'], 'counts': receipt['counts'], 'local_captures_checked': args.local_captures}))


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, StopIteration, OSError, subprocess.CalledProcessError) as error:
        print('FAIL: ' + str(error), file=sys.stderr)
        sys.exit(1)
