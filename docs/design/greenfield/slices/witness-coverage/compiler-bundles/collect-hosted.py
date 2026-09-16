from pathlib import Path
from datetime import datetime
import gzip, hashlib, json, subprocess

CANDIDATE = '160161683d002a18f939f8ee6fcce5e33f6e6faf'
ROOT = Path(__file__).resolve().parents[6]
OUT = ROOT / 'docs/design/greenfield/slices/witness-coverage/compiler-bundles'
RUNS = (35098305041, 35098304984)
EXPECTED = {'acceptance (early)', 'acceptance (wide)', 'acceptance (late)',
            'witnesses (primary)', 'witnesses (controls)', 'witnesses (retained)', 'witnesses (printer)'}

def gh(*args):
    return subprocess.check_output(['gh', *args], cwd=ROOT)

runs = [json.loads(gh('run', 'view', str(run), '--json',
                     'status,conclusion,jobs,headSha,headBranch,event,createdAt,updatedAt,url')) for run in RUNS]
for run in runs:
    assert run['headSha'] == CANDIDATE, run
    assert run['status'] == 'completed' and run['conclusion'] == 'success', run['url']
jobs = [job for run in runs for job in run['jobs'] if job['name'] in EXPECTED]
assert len(jobs) == 7 and {job['name'] for job in jobs} == EXPECTED
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
    if job['name'] == 'witnesses (controls)':
        control_log = raw.decode()
summary_lines = [line[line.index('{"compiler_direct":'):] for line in control_log.splitlines() if '{"compiler_direct":' in line]
assert len(summary_lines) == 1
summary = json.loads(summary_lines[0])
assert summary['targets'] == 18 and summary['tests_passed'] == 64, summary
assert {'bundle-program', 'bundle-declarations'} <= set(summary['compiler_direct'])
for name in (
    'bundle_later_owner_references_remain_separate',
    'fresh_forced_bundle_programs_match_complete_typescript_observations',
    'ordinary_bundle_program_commands_match_complete_typescript_observations',
    'same_session_bundle_commands_getters_and_forces_match_typescript',
    'ordinary_declaration_bundles_match_typescript_visitor_and_printer_twice',
    'ordinary_bundle_source_maps_match_complete_typescript_maps_twice',
    'ordinary_and_fresh_forced_bundle_metadata_lifetimes_match_typescript_twice',
):
    assert f'test {name} ... ok' in control_log, name
receipt = {'version': 1, 'candidate_commit': CANDIDATE,
           'pr': 'https://github.com/kazhiramatsu/tsc-rs/pull/546',
           'workflows': runs, 'replay_jobs': jobs,
           'replay_total_seconds': sum(job['seconds'] for job in jobs),
           'replay_longest_seconds': max(job['seconds'] for job in jobs),
           'compiler_direct': summary,
           'new_coverage': {'suites': ['bundle-program', 'bundle-declarations'], 'rust_tests': 7,
                            'fixture_memberships': [27, 56], 'repetitions': 2,
                            'memberships_are_not_additive_corpus_admissions': True},
           'workers': 2, 'controls_timeout_minutes': 60,
           'timing_excludes': ['plan', 'aggregate gates', 'main push']}
path = OUT / 'hosted.v1.json'
assert not path.exists(), 'refusing to overwrite receipt'
path.write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps({'all_jobs_success': True, 'total_seconds': receipt['replay_total_seconds'],
                  'longest_seconds': receipt['replay_longest_seconds'], 'compiler_direct': summary}))
