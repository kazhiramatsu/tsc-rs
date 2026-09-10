#!/usr/bin/env python3
"""Independently check the external decorator draft's saved command captures.

This authenticates saved observations against the frozen full60 inputs. It
does not attest which executable produced them or qualify production code.
"""
from pathlib import Path
import collections
import hashlib
import json
import re
import tarfile

ROOT = Path(__file__).resolve().parent.parent
PREDECESSOR = Path('ratchets/h2-8a-literal-property-design-experiment.v1.json')
PREDECESSOR_SHA = '03419eb53896a349a2e6e1b6db11f52a00621076140d2e5d11c926cb391d9279'
HANDOFF = ROOT / 'target/h2-8a-decorator-receiver-frames-draft'
FIXTURES = ['retained-accessor-owners', 'class-helper-accessor-producers',
            'class-field-alias-map-positions', 'decorator-receiver-context',
            'retained-lexical-environments', 'retained-constructor-references',
            'retained-lexical-edges', 'retained-comma-factory']


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def read(path):
    return json.loads(Path(path).read_text())


def collect(directory, expected, receipts=None):
    groups = collections.defaultdict(list)
    pins = []
    for path in sorted(directory.glob('*.json')):
        capture = read(path)
        cid = capture['case_id']
        assert cid in expected, cid
        assert capture['expected'] == expected[cid], cid
        assert capture['capture_kind'] == 'supplemental-complete-command'
        pin = {'file': path.name, 'case_id': cid,
               'capture_index': capture['capture_index'], 'sha256': sha(path)}
        pins.append(pin)
        groups[cid].append(capture)
    assert set(groups) == set(expected)
    assert len(pins) == 2 * len(expected)
    if receipts is not None:
        assert pins == sorted(receipts, key=lambda item: item['file'])
    for cid, captures in groups.items():
        captures.sort(key=lambda capture: capture['capture_index'])
        assert [capture['capture_index'] for capture in captures] == [0, 1]
        assert {k: v for k, v in captures[0].items() if k != 'capture_index'} == {
            k: v for k, v in captures[1].items() if k != 'capture_index'}, cid
    return {cid: captures[0] for cid, captures in groups.items()}, pins


def exact(capture):
    return (capture['actual'] is not None and capture['error'] is None
            and capture['partial_writes'] is None
            and capture['actual'] == capture['expected'])


def check_log(path, captures):
    log = path.read_text()
    attempts = collections.Counter(re.findall(
        r'retained accessor owners PRIMARY ATTEMPT (\S+)', log))
    assert attempts == {cid: 2 for cid in captures}
    passes = re.findall(r'retained accessor owners EXACT x2 (\S+)', log)
    failures = re.findall(r'retained accessor owners REPEATED FAILURE (\S+)', log)
    assert len(passes) == len(set(passes))
    assert len(failures) == len(set(failures))
    assert set(passes) == {cid for cid, c in captures.items() if exact(c)}
    assert set(failures) == set(captures) - set(passes)


def main():
    output = ROOT / 'ratchets/h2-8a-decorator-receiver-handoff-review.v1.json'
    assert not output.exists(), 'completed review evidence is immutable'
    assert sha(ROOT / PREDECESSOR) == PREDECESSOR_SHA
    before = read(ROOT / PREDECESSOR)
    source_archive = Path(before['archive']) / 'source-and-inputs.tar.gz'
    assert sha(source_archive) == before['source_archive_sha256']
    pins = {r['path']: r['sha256'] for r in before['copied_inputs']}
    expected, fixture_pins = {}, []
    with tarfile.open(source_archive) as archive:
        for name in FIXTURES:
            relative = f'crates/compiler/tests/fixtures/{name}.json'
            data = archive.extractfile(relative).read()
            assert hashlib.sha256(data).hexdigest() == pins[relative]
            assert (ROOT / relative).read_bytes() == data
            fixture_pins.append({'path': relative, 'sha256': pins[relative]})
            for case in json.loads(data)['cases']:
                if name == 'class-field-alias-map-positions' and case['options']['target'] != 9:
                    continue
                cid = case['case_id']
                assert cid not in expected
                expected[cid] = case['typescript_observation']
    assert len(expected) == 530
    previous, _ = collect(Path(before['archive']) / 'captures', expected,
                          before['capture_receipts'])
    assert {cid for cid, c in previous.items() if exact(c)} == set(before['exact_twice'])
    supplied = read(HANDOFF / 'analysis.json')
    current, captures = collect(HANDOFF / 'captures', expected, supplied['capture_receipts'])
    check_log(HANDOFF / 'run-all.log', current)
    assert sha(HANDOFF / 'run-all.log') == supplied['run_log_sha256']
    assert all(exact(c) for c in current.values())
    changed = sorted(cid for cid in current if any(
        current[cid][k] != previous[cid][k] for k in ['actual', 'error', 'partial_writes']))
    assert changed == sorted(before['failed_twice'])
    patches = []
    for name, digest in supplied['patches'].items():
        path = ROOT / 'docs/design/greenfield/slices' / name
        assert sha(path) == digest == sha(HANDOFF / name)
        patches.append({'path': str(path.relative_to(ROOT)), 'sha256': digest})
    draft_files = []
    for relative, digest in supplied['draft_files'].items():
        assert sha(Path('/Users/hiramatsu/dev/tsc-rs-dec53') / relative) == digest
        draft_files.append({'path': relative, 'sha256': digest})

    # Finish the earlier local prototype's already-terminal context run.
    prefix = ROOT / 'target/h2-8a-comma-printer-design-experiment-61'
    pre_path, exit_path = prefix.with_suffix('.pre.json'), prefix.with_suffix('.exit.json')
    pre, result = read(pre_path), read(exit_path)
    assert pre['selection'] == 'context'
    assert result['manifest_sha256'] == sha(pre_path)
    assert result['log_sha256'] == sha(prefix.with_suffix('.log'))
    assert result['production_unchanged'] and result['copied_inputs_unchanged']
    assert result['root_test_additions_still_absent']
    assert len(result['binaries']) == 1
    for binary in result['binaries']:
        assert sha(binary['retained_path']) == binary['sha256']
    archive61 = Path(pre['archive'])
    assert sha(archive61 / 'source-and-inputs.tar.gz') == pre['source_archive_sha256']
    context = {cid: value for cid, value in expected.items()
               if cid.startswith('decorator-receiver-context/')}
    assert len(context) == 36
    with tarfile.open(archive61 / 'source-and-inputs.tar.gz') as archive:
        for row in pre['inputs'] + pre['vendor_inputs']:
            assert hashlib.sha256(archive.extractfile(row['path']).read()).hexdigest() == row['sha256']
        for pin in fixture_pins:
            assert hashlib.sha256(archive.extractfile(pin['path']).read()).hexdigest() == pin['sha256']
    local, local_captures = collect(archive61 / 'captures', context)
    check_log(prefix.with_suffix('.log'), local)
    local_exact = sorted(cid for cid, c in local.items() if exact(c))
    local_failed = sorted(set(local) - set(local_exact))
    assert result['actual_exit'] == (101 if local_failed else 0)
    review = {
        'version': 1, 'slice': 'H2.8a-A6-41',
        'status': 'independently checked saved observations; isolated draft, not production qualification',
        'predecessor': {'path': str(PREDECESSOR), 'sha256': PREDECESSOR_SHA},
        'source_archive_sha256': sha(source_archive), 'fixtures': fixture_pins,
        'handoff_analysis_sha256': sha(HANDOFF / 'analysis.json'),
        'handoff_run_log_sha256': sha(HANDOFF / 'run-all.log'),
        'summary': {'eligible': 530, 'exact_twice': 530, 'failed_twice': 0,
                    'prior_positives_unchanged': 477, 'repaired': 53, 'regressions': 0},
        'repaired': changed, 'patches': patches, 'draft_files': draft_files,
        'capture_directory': str(HANDOFF / 'captures'), 'capture_receipts': captures,
        'provenance_limit': 'The handoff lacks a launch manifest and retained executed binary. Its final routing predicate was edited after the reported run. A new isolated run will bind the final candidate sources and binary to the observations.',
        'local_prototype_61': {
            'prelaunch': {'path': str(pre_path.relative_to(ROOT)), 'sha256': sha(pre_path)},
            'exit': result, 'archive': str(archive61),
            'source_archive_sha256': pre['source_archive_sha256'],
            'exact_twice': local_exact, 'failed_twice': local_failed,
            'repaired_from_full60': sorted(set(local_exact) & set(before['failed_twice'])),
            'regressions_from_full60': sorted(set(local_failed) & set(before['exact_twice'])),
            'capture_receipts': local_captures,
            'disposition': 'retained for comparison; superseded by the external draft, no further implementation on v17'
        },
        'review_script_sha256': sha(__file__),
        'scope_limit': 'This population is not the whole emitter or H2.8a-e completion inventory.'
    }
    with output.open('x') as stream:
        json.dump(review, stream, indent=2)
        stream.write('\n')
    print(json.dumps({'path': str(output.relative_to(ROOT)), 'sha256': sha(output),
                      'handoff': review['summary'],
                      'local61': {'exact_twice': len(local_exact), 'failed_twice': len(local_failed)}}))


if __name__ == '__main__':
    main()
