#!/usr/bin/env python3
"""Freeze full comma-printer design results and compare both prior candidates."""
from pathlib import Path
import base64
import collections
import difflib
import hashlib
import json
import re
import shutil
import sys

ROOT = Path(__file__).resolve().parent.parent


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read(path):
    return json.loads(Path(path).read_text())


def main():
    attempt = int(sys.argv[1])
    prefix = ROOT / f'target/h2-8a-comma-printer-design-experiment-{attempt}'
    output = prefix.with_suffix('.analysis.json')
    diff_path = prefix.with_suffix('.diff.txt')
    assert not output.exists() and not diff_path.exists(), 'completed analyses are immutable'
    pre_path = prefix.with_suffix('.pre.json')
    pre, result = read(pre_path), read(prefix.with_suffix('.exit.json'))
    log_path = prefix.with_suffix('.log')
    assert result['manifest_sha256'] == sha(pre_path)
    assert result['log_sha256'] == sha(log_path)
    assert result['production_unchanged'] and result['copied_inputs_unchanged']
    assert result['actual_exit'] in [0, 101]
    assert len(result['binaries']) == 1
    for binary in result['binaries']:
        assert sha(binary['retained_path']) == binary['sha256']
    assert pre['selection'] == 'all'
    archive = Path(pre['archive'])
    assert sha(archive / 'source-and-inputs.tar.gz') == pre['source_archive_sha256']
    baseline_path = ROOT / pre['baseline']['path']
    assert sha(baseline_path) == pre['baseline']['sha256']
    baseline = read(baseline_path)
    cases = []
    for name in ['retained-accessor-owners', 'class-helper-accessor-producers',
                 'class-field-alias-map-positions', 'decorator-receiver-context',
                 'retained-lexical-environments', 'retained-constructor-references',
                 'retained-lexical-edges', 'retained-comma-factory']:
        fixture = read(ROOT / f'crates/compiler/tests/fixtures/{name}.json')
        cases.extend(c for c in fixture['cases'] if name != 'class-field-alias-map-positions' or c['options']['target'] == 9)
    expected = {c['case_id']: c['typescript_observation'] for c in cases}
    assert len(cases) == len(expected) == baseline['eligible'] == 530
    previous = collections.defaultdict(list)
    previous_receipts = []
    for band in baseline['bands']:
        native = band['native']
        for receipt in native['capture_receipts']:
            path = Path(native['archive']) / 'captures' / receipt['file']
            assert sha(path) == receipt['sha256']
            capture = read(path)
            assert capture['case_id'] == receipt['case_id']
            assert capture['capture_index'] == receipt['capture_index']
            assert capture['expected'] == expected[capture['case_id']]
            previous[capture['case_id']].append(capture)
            previous_receipts.append({'path': str(path), 'sha256': receipt['sha256']})
    assert len(previous_receipts) == 1060 and set(previous) == set(expected)
    for case_id, captures in previous.items():
        captures.sort(key=lambda c: c['capture_index'])
        assert [c['capture_index'] for c in captures] == [0, 1]
        assert {k: v for k, v in captures[0].items() if k != 'capture_index'} == {k: v for k, v in captures[1].items() if k != 'capture_index'}
        assert captures[0]['actual'] is not None and captures[0]['error'] is None
        assert (captures[0]['actual'] == expected[case_id]) == (case_id in baseline['exact_twice'])
    log = log_path.read_text()
    exact = re.findall(r'retained accessor owners EXACT x2 (\S+)', log)
    failed = re.findall(r'retained accessor owners REPEATED FAILURE (\S+)', log)
    assert len(exact) == len(set(exact)) and len(failed) == len(set(failed))
    assert not set(exact) & set(failed) and set(exact + failed) == set(expected)
    assert result['actual_exit'] == (101 if failed else 0)
    attempts = collections.Counter(re.findall(r'retained accessor owners PRIMARY ATTEMPT (\S+)', log))
    assert attempts == {c: 2 for c in expected}
    groups, receipts = collections.defaultdict(list), []
    for path in sorted((archive / 'captures').glob('*.json')):
        capture = read(path)
        case_id = capture['case_id']
        assert capture['expected'] == expected[case_id]
        assert capture['capture_kind'] == 'supplemental-complete-command'
        groups[case_id].append(capture)
        receipts.append({'file': path.name, 'case_id': case_id,
                         'capture_index': capture['capture_index'], 'sha256': sha(path)})
    assert len(receipts) == 1060 and set(groups) == set(expected)
    rows, differences, typed, changed_from_before = [], [], [], []
    for case_id in sorted(expected):
        captures = sorted(groups[case_id], key=lambda c: c['capture_index'])
        assert [c['capture_index'] for c in captures] == [0, 1]
        assert {k: v for k, v in captures[0].items() if k != 'capture_index'} == {k: v for k, v in captures[1].items() if k != 'capture_index'}
        actual, reference = captures[0]['actual'], expected[case_id]
        same = actual is not None and actual == reference
        assert same == (case_id in exact), case_id
        row = {'case_id': case_id, 'exact_twice': same, 'error': captures[0]['error'],
               'changed_top_level_fields': [], 'writes': []}
        before_actual = previous[case_id][0]['actual']
        row['actual_unchanged_from_before'] = actual == before_actual
        row['changed_fields_from_before'] = sorted(k for k in actual.keys() | before_actual.keys() if actual.get(k) != before_actual.get(k)) if actual is not None else None
        if not row['actual_unchanged_from_before']:
            changed_from_before.append(case_id)
        if actual is None:
            typed.append(case_id)
            differences.append(f'\n{case_id}\nTyped failure: {captures[0]["error"]}\n')
        elif not same:
            row['changed_top_level_fields'] = sorted(k for k in actual.keys() | reference.keys() if actual.get(k) != reference.get(k))
            differences.append('\n' + case_id + '\nChanged: ' + ', '.join(row['changed_top_level_fields']) + '\n')
            actual_writes, expected_writes = ({w['path']: w for w in value['writes']} for value in [actual, reference])
            assert len(actual_writes) == len(actual['writes']) and len(expected_writes) == len(reference['writes'])
            for path in sorted(actual_writes.keys() | expected_writes.keys()):
                left, right = actual_writes.get(path), expected_writes.get(path)
                if left == right:
                    continue
                item = {'path': path, 'actual_present': left is not None, 'expected_present': right is not None}
                if left is not None and right is not None:
                    item['changed_fields'] = sorted(k for k in left.keys() | right.keys() if left.get(k) != right.get(k))
                a_text, e_text = (base64.b64decode(value['callback_utf8_base64'], validate=True).decode('utf8') if value else '' for value in [left, right])
                if left is not None and right is not None and path.endswith('.map'):
                    am, em = json.loads(a_text), json.loads(e_text)
                    item['differing_map_keys'] = sorted(k for k in am.keys() | em.keys() if am.get(k) != em.get(k))
                row['writes'].append(item)
                differences.extend(difflib.unified_diff(a_text.splitlines(keepends=True), e_text.splitlines(keepends=True), fromfile='actual:' + path, tofile='expected:' + path))
        rows.append(row)
    required = set(baseline['required_repair_ids'])
    prior_exact = set(baseline['exact_twice'])
    successor_ids = set(baseline['failed_twice']) - required
    summary = {'eligible': 530, 'exact_twice': len(exact), 'failed_twice': len(failed),
               'required_repaired': len(required & set(exact)), 'required_remaining': len(required & set(failed)),
               'prior_preserved': len(prior_exact & set(exact)), 'regressions': len(prior_exact & set(failed)),
               'successor_exact': len(successor_ids & set(exact)), 'successor_failed': len(successor_ids & set(failed)),
               'successor_changed_from_before': len(successor_ids & set(changed_from_before)),
               'typed_failures': len(typed), 'actual_exit': result['actual_exit']}
    if pre.get('list_owner_patch') is not None:
        predecessors = {
            'f391fb4dade7a634aa92c998fdb6530c979d1a34debef984a8d75a0f522b9b7a': (
                'ratchets/h2-8a-import-type-attributes-design-experiment.v1.json',
                'e1943a38e9001618e2a39fe0bb28c6b7548008282b67048dae36659a727d9aeb'),
            'c787c9e6ab327bd355b2ae9d8a9ac47e761324c93d1032569053c5bc8464f50a': (
                'ratchets/h2-8a-list-format-flags-design-experiment.v1.json',
                '2cbdd4004a7c0f2ef46e241bedf1155c45aca6f776550860cbb2af586bbd89ab'),
            'c9f84f82ccf3ba8eadcb50bb585342693c7a8128e91a8c4190581e55298e6805': (
                'ratchets/h2-8a-list-format-flags-design-experiment.v1.json',
                '2cbdd4004a7c0f2ef46e241bedf1155c45aca6f776550860cbb2af586bbd89ab'),
            'fea19f9b360900530d5f6f29847d2ae9f005b7ae6715b4b2ac9cdae9b7e02552': (
                'ratchets/h2-8a-list-cursor-design-experiment.v1.json',
                'e82ef777e0647b876e8de7aa60b2ea317f42a5c8c6d8fcc0e132e59fc136bbbb'),
            'b72e45e6317307a04d0576ba15cc000c50296cdd07c1c8e0476a67ae4d54083a': (
                'ratchets/h2-8a-list-cursor-design-experiment.v1.json',
                'e82ef777e0647b876e8de7aa60b2ea317f42a5c8c6d8fcc0e132e59fc136bbbb'),
            '271c5668f7de149c8d14f9b71173e4e744551b7fa9a094f47950485e38a7e697': (
                'ratchets/h2-8a-list-cursor-design-experiment.v1.json',
                'e82ef777e0647b876e8de7aa60b2ea317f42a5c8c6d8fcc0e132e59fc136bbbb'),
            'e22b80f928d7d307aad96a3c3f58fdb8f161c3ede7079c00c6b3e1a0cb1f602c': (
                'ratchets/h2-8a-list-boundary-lines-design-experiment.v1.json',
                '98142a8b7b575bac8d3378b2696a52315577466fca2b271320013fc673ff6e43'),
            '805a14b709ad3ec858181ed94b1399ab6e4f24c9a24c33207fd8d237ebfd97d2': (
                'ratchets/h2-8a-list-trailing-token-design-experiment.v1.json',
                'fc140aa5aca03bb3b35bd0eb5f67818de8fa1f81f54bc776707758c406f06a7f'),
            'dc9df77bbe90b4a588decd33de9ac9c0ddfa2561929212aa34b175838342ba5c': (
                'ratchets/h2-8a-list-trailing-token-design-experiment.v1.json',
                'fc140aa5aca03bb3b35bd0eb5f67818de8fa1f81f54bc776707758c406f06a7f'),
            'e0b8c978b40d1dc19e834f25403681a35ebd8d0de11bea4d1f2caca119d1446d': (
                'ratchets/h2-8a-comma-argument-factory-design-experiment.v1.json',
                '56ce6ff67fa17c9d5992f480d3e32adbe6c88ff35d79913208c83960080d9043'),
            '37d3637aa327c87a3ed7529d88a0c63ad9c374342a49af554917a80bb4fb78a3': (
                'ratchets/h2-8a-list-intervening-design-experiment.v1.json',
                'bd8b77e5d69ab95b1869de03f514640074530e6c25bf7bc7fea426c3b9f8ed0f'),
        }
        candidate_patch = pre['list_owner_patch']['sha256']
        assert candidate_patch in predecessors, 'pin the strongest completed predecessor for this new patch'
        predecessor_relative, predecessor_sha = predecessors[candidate_patch]
        predecessor_path = ROOT / predecessor_relative
    elif pre.get('factory_patch') is not None:
        predecessor_path = ROOT / 'ratchets/h2-8a-comma-printer-design-experiment.v1.json'
        predecessor_sha = 'b2f8e916afecc13a66c81e96faef3073c879ba92f1feefbdf81b70170f87839d'
    else:
        predecessor_path = ROOT / 'ratchets/h2-8a-retained-lexical-design-experiment.v1.json'
        predecessor_sha = '056a3c1d951d59a96458f7128310c37772e18fc54db069924f06ac63b158f215'
    assert sha(predecessor_path) == predecessor_sha
    predecessor = read(predecessor_path)
    predecessor_groups = collections.defaultdict(list)
    predecessor_receipts = []
    for receipt in predecessor['capture_receipts']:
        path = Path(predecessor['archive']) / 'captures' / receipt['file']
        assert sha(path) == receipt['sha256']
        capture = read(path)
        assert capture['case_id'] == receipt['case_id']
        assert capture['capture_index'] == receipt['capture_index']
        assert capture['expected'] == expected[capture['case_id']]
        predecessor_groups[capture['case_id']].append(capture)
        predecessor_receipts.append({'path': str(path), 'sha256': receipt['sha256']})
    assert len(predecessor_receipts) == 1060 and set(predecessor_groups) == set(expected)
    candidate_changed = []
    for case_id, captures in predecessor_groups.items():
        captures.sort(key=lambda c: c['capture_index'])
        assert [c['capture_index'] for c in captures] == [0, 1]
        first = {k: v for k, v in captures[0].items() if k != 'capture_index'}
        second = {k: v for k, v in captures[1].items() if k != 'capture_index'}
        assert first == second
        assert (captures[0]['actual'] == expected[case_id]) == (case_id in predecessor['exact_twice'])
        current = sorted(groups[case_id], key=lambda c: c['capture_index'])[0]
        if any(current[k] != captures[0][k] for k in ['actual', 'error', 'partial_writes']):
            candidate_changed.append(case_id)
    preserved = set(predecessor['exact_twice']) & set(exact)
    candidate_regressions = set(predecessor['exact_twice']) - set(exact)
    summary.update({'candidate_preserved': len(preserved), 'candidate_regressions': len(candidate_regressions),
                    'changed_from_candidate': len(candidate_changed)})
    record = {'version': 1, 'slice': 'H2.8a-A6-40', 'status': 'isolated design experiment; not a qualified after profile',
              'summary': summary, 'exact_twice': sorted(exact), 'failed_twice': sorted(failed),
              'required_repaired': sorted(required & set(exact)), 'required_remaining': sorted(required & set(failed)),
              'regressions': sorted(prior_exact & set(failed)), 'successor_exact': sorted(successor_ids & set(exact)),
              'successor_failed': sorted(successor_ids & set(failed)), 'typed_boundary_cases': typed,
              'changed_from_before': sorted(changed_from_before),
              'successor_changed_from_before': sorted(successor_ids & set(changed_from_before)),
              'previous_capture_receipts': previous_receipts,
              'primary_attempts': sum(attempts.values()), 'supplemental_executions': len(receipts),
              'repetition_requirement_satisfied': True, 'rows': rows, 'capture_receipts': receipts,
              'predecessor': {'path': str(predecessor_path.relative_to(ROOT)), 'sha256': sha(predecessor_path)},
              'predecessor_capture_receipts': predecessor_receipts,
              'candidate_regressions': sorted(candidate_regressions), 'changed_from_candidate': sorted(candidate_changed),
              'printer_patch_sha256': pre['printer_patch_sha256'], 'factory_patch': pre.get('factory_patch'),
              'list_owner_patch': pre.get('list_owner_patch'),
              'prelaunch': {'path': str(pre_path), 'sha256': sha(pre_path)}, 'exit': result,
              'baseline': pre['baseline'], 'candidate_sha256': pre['candidate_sha256'], 'archive': str(archive),
              'global_claim': 'The overlapping original769/class1228 populations were not rerun; H2.8a-e and A40 readiness remain open.'}
    output.write_text(json.dumps(record, indent=2) + '\n')
    diff_path.write_text(''.join(differences))
    for source, name in [(output, 'analysis.json'), (diff_path, 'differences.txt'), (Path(__file__), 'analyze.py')]:
        assert not (archive / name).exists()
        shutil.copy2(source, archive / name)
    print(json.dumps(summary))


if __name__ == '__main__':
    main()
