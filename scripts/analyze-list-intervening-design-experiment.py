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
    assert len(terminal['binaries']) == 10
    source_archive = archive / 'source-and-inputs.tar.gz'
    assert sha(source_archive.read_bytes()) == pre['source_archive_sha256']
    fixtures = [
        ('crates/emitter/tests/fixtures/comma-list-printer.json', 50),
        ('crates/emitter/tests/fixtures/comma-argument-factory.json', 44),
        ('crates/emitter/tests/fixtures/list-intervening-owners.json', 96),
        ('crates/emitter/tests/fixtures/list-trailing-token-owners.json', 104),
        ('crates/emitter/tests/fixtures/list-boundary-lines.json', 264),
        ('ratchets/h2-8a-list-cursor-lifecycle.v1.json', 11),
        ('crates/emitter/tests/fixtures/mapped-type-members.json', 328),
        ('crates/emitter/tests/fixtures/list-format-flags.json', 160),
        ('crates/emitter/tests/fixtures/import-type-attributes.json', 84),
        ('crates/emitter/tests/fixtures/emit-pipeline-phases.json', 8),
        ('crates/emitter/tests/fixtures/emit-pipeline-bundle.json', 2),
        ('crates/emitter/tests/fixtures/literal-parent-provenance-utf16.json', 128),
        ('crates/emitter/tests/fixtures/utf16-literal-escaping.json', 288),
        ('crates/emitter/tests/fixtures/utf16-writer.json', 48),
        ('crates/emitter/tests/fixtures/template-raw-provenance.json', 480),
        ('crates/emitter/tests/fixtures/string-property-provenance.json', 60),
    ]
    cases = {}
    artifacts = []
    with tarfile.open(source_archive, 'r:gz') as tar:
        for row in pre['inputs'] + pre['vendor_inputs']:
            assert sha(tar.extractfile(row['path']).read()) == row['sha256'], row['path']
        for path, count in fixtures:
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
    final_lists = re.findall(r'(?:comma argument factory|comma list printer|list cursor|mapped type members|list format flags|import type attributes|emit pipeline phases|literal parent provenance|utf16 literal escaping|utf16 writer|template raw provenance|string property provenance) failures: (\[[^\n]*\])', log)
    final_failures = {entry for rendered in final_lists for entry in json.loads(rendered)}
    assert final_failures == {f'{case_id} repetition {rep}' for case_id, rep in failures}
    for test in ['comma_argument_factory_matches_typescript', 'comma_list_printer_matches_typescript', 'list_cursor_lifecycle_matches_typescript', 'mapped_type_members_matches_typescript', 'list_format_flags_matches_typescript', 'import_type_attributes_matches_typescript', 'emit_pipeline_phases_matches_typescript', 'literal_parent_provenance_matches_typescript', 'utf16_literal_escaping_matches_typescript', 'utf16_writer_matches_typescript', 'writer_value_equality_survives_chunking_and_clone_clear', 'template_raw_provenance_matches_typescript', 'string_property_provenance_matches_typescript']:
        assert f'test {test} ...' in log
    assert terminal['actual_exit'] == (101 if failures else 0)
    assert len(re.findall(r'test result: (?:ok|FAILED)\.', log)) == 10
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
        if 'tree_state' in case['typescript_observation']:
            row['tree_state_exact_twice'] = pair[0] is None or pair[0]['tree_state'] == case['typescript_observation']['tree_state']
        if 'events' in case['typescript_observation']:
            row['events_exact_twice'] = pair[0] is None or pair[0]['events'] == case['typescript_observation']['events']
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
                    'factory_state_exact_twice': sum(row.get('factory_state_exact_twice', False) for row in rows),
                    'tree_state_exact_twice': sum(row.get('tree_state_exact_twice', False) for row in rows),
                    'events_exact_twice': sum(row.get('events_exact_twice', False) for row in rows)},
        'results': rows,
    }
    with output.open('x') as stream:
        json.dump(result, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'output': str(output), 'sha256': sha(output.read_bytes()), **result['summary']}))


if __name__ == '__main__':
    main()
