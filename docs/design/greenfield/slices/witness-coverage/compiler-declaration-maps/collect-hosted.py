from pathlib import Path
from datetime import datetime
import gzip, hashlib, json, subprocess

CANDIDATE = '6cfd82ca224b01c61764ae5ccf78457841027324'
ROOT = Path(__file__).resolve().parents[6]
OUT = ROOT / 'docs/design/greenfield/slices/witness-coverage/compiler-declaration-maps'
RUNS = (35119769009, 35119768954)
EXPECTED = {'acceptance (early)', 'acceptance (wide)', 'acceptance (late)',
            'witnesses (primary)', 'witnesses (controls)', 'witnesses (retained)', 'witnesses (printer)',
            'witnesses (declaration-maps)'}

def gh(*args):
    return subprocess.check_output(['gh', *args], cwd=ROOT)

runs = [json.loads(gh('run', 'view', str(run), '--json',
                     'status,conclusion,jobs,headSha,headBranch,event,createdAt,updatedAt,url')) for run in RUNS]
for run in runs:
    assert run['headSha'] == CANDIDATE, run
    assert run['status'] == 'completed' and run['conclusion'] == 'success', run['url']
jobs = [job for run in runs for job in run['jobs'] if job['name'] in EXPECTED]
assert len(jobs) == 8 and {job['name'] for job in jobs} == EXPECTED
all_jobs = [job for run in runs for job in run['jobs']]
for gate in ('gates', 'witness-gates'):
    rows = [job for job in all_jobs if job['name'] == gate]
    assert len(rows) == 1 and rows[0]['conclusion'] == 'success', gate
for job in jobs:
    assert job['conclusion'] == 'success', job
    raw = gh('api', f"repos/kazhiramatsu/tsc-rs/actions/jobs/{job['databaseId']}/logs")
    name = f"hosted-{job['databaseId']}.log.gz"
    path = OUT / name
    compressed = gzip.compress(raw, mtime=0)
    if path.exists():
        assert path.read_bytes() == compressed, name
    else:
        path.write_bytes(compressed)
    job['log'] = name
    job['log_sha256'] = hashlib.sha256(compressed).hexdigest()
    job['log_uncompressed_sha256'] = hashlib.sha256(raw).hexdigest()
    parse = lambda time: datetime.fromisoformat(time.replace('Z', '+00:00'))
    job['seconds'] = int((parse(job['completedAt']) - parse(job['startedAt'])).total_seconds())
    if job['name'] == 'witnesses (declaration-maps)':
        control_log = raw.decode()
summary_lines = [line[line.index('{"compiler_direct":'):] for line in control_log.splitlines() if '{"compiler_direct":' in line]
assert len(summary_lines) == 1
summary = json.loads(summary_lines[0])
assert summary['targets'] == 2 and summary['tests_passed'] == 11, summary
assert summary['compiler_direct'] == ['declaration-map-apis', 'declaration-maps'], summary
assert control_log.count('H2.7e CLI PASS') == 70
for target, count in [('h2_7e_declaration_map_apis', 3), ('h2_7e_declaration_maps', 8)]:
    assert f'Running tests/{target}.rs' in control_log, target
    assert f'test result: ok. {count} passed; 0 failed; 0 ignored;' in control_log, count
control_job = next(job for job in jobs if job['name'] == 'witnesses (controls)')
original_controls = gzip.decompress((OUT / control_job['log']).read_bytes()).decode()
control_summaries = [json.loads(line[line.index('{"compiler_direct":'):]) for line in original_controls.splitlines() if '{"compiler_direct":' in line]
assert len(control_summaries) == 1 and control_summaries[0]['targets'] == 18 and control_summaries[0]['tests_passed'] == 64
receipt = {'version': 1, 'candidate_commit': CANDIDATE,
           'pr': 'https://github.com/kazhiramatsu/tsc-rs/pull/547',
           'workflows': runs, 'replay_jobs': jobs,
           'replay_total_seconds': sum(job['seconds'] for job in jobs),
           'replay_longest_seconds': max(job['seconds'] for job in jobs),
           'compiler_direct': summary, 'retained_controls_compiler_direct': control_summaries[0],
           'new_coverage': {'suites': ['declaration-map-apis', 'declaration-maps'], 'rust_tests': 11,
                            'fixture_memberships': [75, 84], 'cli_comparison_pairs': 70, 'repetitions': 2,
                            'memberships_are_not_additive_corpus_admissions': True},
           'workers': 2, 'controls_timeout_minutes': 60,
           'timing_excludes': ['plan', 'aggregate gates', 'main push']}
path = OUT / 'hosted.v1.json'
assert not path.exists(), 'refusing to overwrite receipt'
path.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps({'all_jobs_success': True, 'total_seconds': receipt['replay_total_seconds'],
                  'longest_seconds': receipt['replay_longest_seconds'], 'compiler_direct': summary}))
