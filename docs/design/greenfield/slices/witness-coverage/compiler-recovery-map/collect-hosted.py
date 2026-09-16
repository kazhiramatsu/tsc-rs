from pathlib import Path
import datetime, hashlib, json, subprocess, re
import argparse
parser = argparse.ArgumentParser(description='Fetch the fixed PR #545 runs and verify coverage counts.')
parser.add_argument('--out', type=Path, required=True)
out = parser.parse_args().out
out.mkdir(parents=True, exist_ok=True)
expected = {'acceptance (early)', 'acceptance (wide)', 'acceptance (late)',
            'witnesses (primary)', 'witnesses (controls)', 'witnesses (retained)', 'witnesses (printer)'}
workflows = []
for run in (35092307686, 35092307896):
    record = json.loads(subprocess.check_output(['gh', 'run', 'view', str(run), '--json',
                       'status,conclusion,jobs,headSha,headBranch,event,createdAt,updatedAt,url'], text=True))
    assert record['headSha'] == '008a678c4455ead5a0276eaa7587d10b4cc7bb2e'
    assert record['status'] == 'completed' and record['conclusion'] == 'success', record['url']
    workflows.append(record)
jobs = [job for run in workflows for job in run['jobs'] if job['name'] in expected]
assert len(jobs) == len(expected) and {job['name'] for job in jobs} == expected
for job in jobs:
    assert job['conclusion'] == 'success'
    log = out / f"job-{job['databaseId']}.log"
    if not log.exists():
        log.write_bytes(subprocess.check_output(['gh', 'api', f"repos/kazhiramatsu/tsc-rs/actions/jobs/{job['databaseId']}/logs"]))
    job['log_sha256'] = hashlib.sha256(log.read_bytes()).hexdigest()
    job['local_log'] = str(log)
    parse = lambda value: datetime.datetime.fromisoformat(value.replace('Z', '+00:00'))
    job['seconds'] = int((parse(job['completedAt']) - parse(job['startedAt'])).total_seconds())
control = next(job for job in jobs if job['name'] == 'witnesses (controls)')
log = Path(control['local_log']).read_text()
summary_line = next(line[line.index('{"compiler_direct":'):] for line in log.splitlines() if '{"compiler_direct":' in line)
summary = json.loads(summary_line)
assert summary['targets'] == 16 and summary['tests_passed'] == 57, summary
assert 'utf16-recovery-corpus' in summary['compiler_direct'] and 'map-option-projection' in summary['compiler_direct']
assert len(re.findall(r'H2\.6a projection EXACT x2', log)) == 67
assert len(re.findall(r': SourceMap verified x2', log)) == 3
assert len(re.findall(r': MapFamily verified x2', log)) == 3
assert 'test newly_admitted_literal_recovery_rows_match_complete_commands_twice ... ok' in log
assert '"cases":50,"skipped":0,"complete_command_executions":100' in log
result = {'version': 1, 'candidate_commit': workflows[0]['headSha'], 'pr': 'https://github.com/kazhiramatsu/tsc-rs/pull/545',
          'workflows': workflows, 'replay_jobs': jobs, 'replay_total_seconds': sum(job['seconds'] for job in jobs),
          'replay_longest_seconds': max(job['seconds'] for job in jobs), 'compiler_direct': summary,
          'new_coverage': {'recovery_complete_commands': 50, 'map_dedicated_inputs': 31, 'map_original_ids': 5,
                           'map_adapter_observations': 62, 'map_original_projection_observations': 5,
                           'map_other_floor_exact_controls': 3, 'map_old_floor_difference_controls': 3,
                           'repetitions': 2}, 'workers': 2, 'job_timeout_minutes': 60,
          'timing_excludes': ['plan', 'aggregate gates', 'main push']}
(out / 'hosted.v1.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({'all_jobs_success': True, 'total_seconds': result['replay_total_seconds'], 'longest_seconds': result['replay_longest_seconds'], 'compiler_direct': summary}))
