"""Archive final-head PR jobs; do not replace an existing completed receipt."""
from datetime import datetime
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[6]
OUT = Path(__file__).resolve().parent / 'hosted'
CANDIDATE = '447920e6c73f9a81d0aae8c2729a395426c9ff3b'
RUNS = (35178344808, 35178344800)
EXPECTED = {'acceptance (early)', 'acceptance (wide)', 'acceptance (late)',
            'witnesses (primary)', 'witnesses (controls)', 'witnesses (retained)',
            'witnesses (printer)', 'witnesses (declaration-maps)',
            'witnesses (decorator-binding-pipeline)', 'witnesses (foundations)'}
sys.path.insert(0, str(ROOT / 'scripts'))
import foundation_witnesses as foundation


def gh(*args):
    return subprocess.check_output(['gh', *args], cwd=ROOT)


def save(path, data):
    if path.exists():
        assert path.read_bytes() == data, f'refusing changed archive: {path}'
    else:
        path.write_bytes(data)


def summary(log, key):
    marker = '{"' + key + '":'
    rows = [json.loads(line[line.index(marker):]) for line in log.splitlines() if marker in line]
    assert len(rows) == 1, (key, len(rows))
    return rows[0]


assert not (OUT / 'receipt.v1.json').exists(), 'refusing to overwrite final receipt'
runs = [json.loads(gh('run', 'view', str(run), '--json',
                     'status,conclusion,jobs,headSha,headBranch,event,createdAt,updatedAt,url')) for run in RUNS]
for run in runs:
    assert run['headSha'] == CANDIDATE and run['event'] == 'pull_request', run['url']
    assert run['status'] == 'completed' and run['conclusion'] == 'success', run['url']
all_jobs = [job for run in runs for job in run['jobs']]
jobs = [job for job in all_jobs if job['name'] in EXPECTED]
assert len(jobs) == 10 and {job['name'] for job in jobs} == EXPECTED
assert len(all_jobs) == 14 and all(job['conclusion'] == 'success' for job in all_jobs)
assert [job['name'] for job in all_jobs].count('plan') == 2
for gate in ('gates', 'witness-gates'):
    assert len([job for job in all_jobs if job['name'] == gate]) == 1
OUT.mkdir(exist_ok=True)
logs = {}
for job in all_jobs:
    raw = gh('api', f"repos/kazhiramatsu/tsc-rs/actions/jobs/{job['databaseId']}/logs")
    name = f"{job['databaseId']}.log.gz"
    compressed = gzip.compress(raw, mtime=0)
    save(OUT / name, compressed)
    job['log'] = name
    job['log_sha256'] = hashlib.sha256(compressed).hexdigest()
    job['log_uncompressed_sha256'] = hashlib.sha256(raw).hexdigest()
    parse = lambda value: datetime.fromisoformat(value.replace('Z', '+00:00'))
    job['seconds'] = int((parse(job['completedAt']) - parse(job['startedAt'])).total_seconds())
    logs[job['name']] = raw.decode()
foundations = summary(logs['witnesses (foundations)'], 'foundation_direct')
assert foundations['platform'] == 'linux' and foundations['targets'] == 16
assert foundations['tests_passed'] == 48
assert set(foundations['foundation_direct']) == set(foundation.SUITES)
for suite in foundation.SUITES:
    assert f"Running tests/{foundation.SUITES[suite]['target']}.rs" in logs['witnesses (foundations)']
binding = summary(logs['witnesses (decorator-binding-pipeline)'], 'suite')
assert binding['suite'] == 'decorator-binding-pipeline'
assert (binding['exact'], binding['known'], binding['selected'], binding['known_frozen']) == (763, 4, 767, 4)
controls = summary(logs['witnesses (controls)'], 'compiler_direct')
assert controls['targets'] == 21 and controls['tests_passed'] == 70, controls
receipt = {'version': 1, 'candidate_commit': CANDIDATE,
           'candidate_tree': gh('api', f'repos/kazhiramatsu/tsc-rs/git/commits/{CANDIDATE}', '--jq', '.tree.sha').decode().strip(),
           'pr': 'https://github.com/kazhiramatsu/tsc-rs/pull/551',
           'workflows': runs, 'replay_jobs': jobs,
           'replay_total_seconds': sum(job['seconds'] for job in jobs),
           'replay_longest_seconds': max(job['seconds'] for job in jobs),
           'foundation_direct': foundations, 'retained_controls_compiler_direct': controls,
           'retained_binding_pipeline': binding,
           'coverage': {'new_targets': 16, 'linux_tests': 48, 'macos_tests': 47,
                        'compiler_command_parity_claim': False, 'windows_run': False},
           'workers': 2, 'foundations_timeout_minutes': 60,
           'timing_excludes': ['plan', 'aggregate gates', 'main push']}
save(OUT / 'receipt.v1.json', (json.dumps(receipt, indent=2) + '\n').encode())
print(json.dumps({'all_14_checks_success': True, 'total_seconds': receipt['replay_total_seconds'],
                  'longest_seconds': receipt['replay_longest_seconds'], 'foundation_direct': foundations}))
