#!/usr/bin/env python3
"""Freeze complete direct-control outcomes from an archived design attempt.

Reads the executed input archive and actual terminal log. It never supplies
native expected output or qualifies production behavior.
"""
from pathlib import Path
import hashlib
import json
import re
import sys
import tarfile

ROOT = Path(__file__).resolve().parent.parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def serde_debug(text):
    """Parse serde_json::Value's Debug format, without eval or substitutions."""
    position = 0

    def consume(spelling):
        nonlocal position
        assert text.startswith(spelling, position), (position, spelling)
        position += len(spelling)

    def whitespace():
        nonlocal position
        while position < len(text) and text[position].isspace():
            position += 1

    def string():
        nonlocal position
        value, length = json.JSONDecoder().raw_decode(text[position:])
        assert isinstance(value, str)
        position += length
        return value

    def value():
        nonlocal position
        whitespace()
        if text.startswith('Object {', position):
            consume('Object {')
            result = {}
            while True:
                whitespace()
                if text.startswith('}', position):
                    consume('}')
                    return result
                key = string()
                assert key not in result
                consume(':')
                result[key] = value()
                whitespace()
                if text.startswith(',', position):
                    consume(',')
                else:
                    consume('}')
                    return result
        if text.startswith('Array [', position):
            consume('Array [')
            result = []
            while True:
                whitespace()
                if text.startswith(']', position):
                    consume(']')
                    return result
                result.append(value())
                whitespace()
                if text.startswith(',', position):
                    consume(',')
                else:
                    consume(']')
                    return result
        if text.startswith('String(', position):
            consume('String(')
            result = string()
            consume(')')
            return result
        if text.startswith('Number(', position):
            consume('Number(')
            end = text.index(')', position)
            result = json.loads(text[position:end])
            assert isinstance(result, (int, float))
            position = end + 1
            return result
        if text.startswith('Bool(', position):
            consume('Bool(')
            end = text.index(')', position)
            result = json.loads(text[position:end])
            assert isinstance(result, bool)
            position = end + 1
            return result
        consume('Null')
        return None

    result = value()
    assert position == len(text), position
    return result


def main():
    attempt = int(sys.argv[1])
    prefix = ROOT / f'target/h2-8a-comma-printer-design-experiment-{attempt}'
    output = prefix.with_suffix('.direct-analysis.json')
    assert not output.exists(), 'completed analysis is immutable'
    pre_bytes = prefix.with_suffix('.pre.json').read_bytes()
    pre = json.loads(pre_bytes)
    terminal_bytes = prefix.with_suffix('.exit.json').read_bytes()
    terminal = json.loads(terminal_bytes)
    log_bytes = prefix.with_suffix('.log').read_bytes()
    log = log_bytes.decode()
    assert pre['selection'] == 'factory-direct'
    assert terminal['manifest_sha256'] == sha(pre_bytes)
    assert terminal['log_sha256'] == sha(log_bytes)
    assert terminal['production_unchanged'] and terminal['copied_inputs_unchanged']
    archive = Path(pre['archive'])
    for name, data in [('prelaunch.json', pre_bytes), ('actual-exit.json', terminal_bytes), ('run.log', log_bytes)]:
        assert (archive / name).read_bytes() == data
    for binary in terminal['binaries']:
        retained = Path(binary['retained_path'])
        assert retained.stat().st_size == binary['size']
        assert sha(retained.read_bytes()) == binary['sha256']
    assert len(terminal['binaries']) == 2
    source_archive = archive / 'source-and-inputs.tar.gz'
    assert sha(source_archive.read_bytes()) == pre['source_archive_sha256']
    fixtures = ['comma-list-printer', 'comma-argument-factory', 'list-intervening-owners',
                'list-trailing-token-owners', 'list-boundary-lines']
    cases = {}
    artifacts = []
    with tarfile.open(source_archive, 'r:gz') as tar:
        for row in pre['inputs'] + pre['vendor_inputs']:
            assert sha(tar.extractfile(row['path']).read()) == row['sha256'], row['path']
        for name, count in zip(fixtures, [50, 44, 96, 104, 264], strict=True):
            path = f'crates/emitter/tests/fixtures/{name}.json'
            data = tar.extractfile(path).read()
            artifact = json.loads(data)
            assert artifact['typescript'] == '6.0.3' and artifact['repetitions'] == 2
            assert len(artifact['cases']) == count
            artifacts.append({'path': path, 'sha256': sha(data), 'cases': count})
            for case in artifact['cases']:
                assert case['case_id'] not in cases
                cases[case['case_id']] = case
        runner_path = 'scripts/run-comma-printer-design-experiment.py'
        runner_sha = sha(tar.extractfile(runner_path).read())
    failures = {}
    pattern = r'assertion `left == right` failed: (\S+) repetition ([01])\n  left: (.+)\n right: (.+)'
    for match in re.finditer(pattern, log):
        case_id, repetition, left, right = match.groups()
        key = (case_id, int(repetition))
        assert key not in failures and case_id in cases
        actual, expected = serde_debug(left), serde_debug(right)
        assert expected == cases[case_id]['typescript_observation'], case_id
        assert actual != expected
        failures[key] = actual
    # The catch-unwind loop's final list includes early typed failures as
    # well as assertion differences; refuse an unparsed panic or missing row.
    final_lists = re.findall(r'(?:comma argument factory|comma list printer) failures: (\[[^\n]*\])', log)
    final_failures = {entry for rendered in final_lists for entry in json.loads(rendered)}
    assert final_failures == {f'{case_id} repetition {rep}' for case_id, rep in failures}
    for test in ['comma_argument_factory_matches_typescript', 'comma_list_printer_matches_typescript']:
        assert f'test {test} ...' in log
    assert terminal['actual_exit'] == (101 if failures else 0)
    assert len(re.findall(r'test result: (?:ok|FAILED)\.', log)) == 2
    rows = []
    for case_id, case in cases.items():
        pair = [failures.get((case_id, rep)) for rep in range(2)]
        assert pair[0] == pair[1], case_id
        row = {'case_id': case_id, 'exact_twice': pair[0] is None}
        if pair[0] is not None:
            row.update(actual=pair[0], expected=case['typescript_observation'])
            if 'array_state' in pair[0]:
                row['factory_state_exact_twice'] = pair[0]['array_state'] == case['typescript_observation']['array_state']
        elif 'array_state' in case['typescript_observation']:
            row['factory_state_exact_twice'] = True
        rows.append(row)
    result = {
        'version': 1, 'attempt': attempt,
        'status': 'Isolated design evidence; no production activation or qualified-after claim',
        'analyzer': {'path': str(Path(__file__).relative_to(ROOT)), 'sha256': sha(Path(__file__).read_bytes())},
        'archived_runner_sha256': runner_sha,
        'prelaunch_sha256': sha(pre_bytes), 'prelaunch': pre,
        'terminal_sha256': sha(terminal_bytes), 'terminal': terminal, 'fixtures': artifacts,
        'summary': {'controls': len(rows), 'exact_twice': sum(row['exact_twice'] for row in rows),
                    'failed_twice': len(failures) // 2, 'native_attempts': len(rows) * 2,
                    'factory_state_exact_twice': sum(row.get('factory_state_exact_twice', False) for row in rows)},
        'results': rows,
    }
    with output.open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'output': str(output), 'sha256': sha(output.read_bytes()), **result['summary']}))


if __name__ == '__main__':
    main()
