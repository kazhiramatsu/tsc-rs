"""Read-only checks shared by final-run and documentation helpers."""
import re
from pathlib import Path


def expected_phases(main):
    phases = set()
    for start, end in [('ci_with_resume', 'resolve_ci_baseline'),
                       ('ci_rust_gates', 'ci_hosted_gates'),
                       ('ci_semantic_gates', 'ci_semantic_preflight')]:
        opening, closing = 'fn ' + start + '(', 'fn ' + end + '('
        assert main.count(opening) == main.count(closing) == 1, 'CI phase anchors changed'
        body = main.split(opening, 1)[1].split(closing, 1)[0]
        phases.update(re.findall(r'resume\.run_phase\(\s*"([^"]+)"', body))
    phases.remove('hosted-diagnostic')
    assert len(phases) == 18, 'unsplit CI phase roster changed'
    return phases


def validate_phase_log(main, log):
    if isinstance(log, bytes):
        log = log.decode(errors='replace')
    phases = expected_phases(main)
    recorded = re.findall(r'^local CI checkpoint: recorded (.+)$', log, re.M)
    reused = re.findall(r'^local CI resume: reuse (.+) \(exact inputs and outputs\)$', log, re.M)
    runs = re.findall(r'^local CI phase: run (.+)$', log, re.M)
    assert len(recorded + reused) == len(set(recorded + reused)), 'duplicate CI phase receipt'
    assert set(recorded + reused) == phases, 'missing or unexpected CI phase receipt'
    assert len(runs) == len(set(runs)) and set(runs) == set(recorded), 'CI runs and recorded phases disagree'
    completion = re.findall(r'^local CI resume: complete; cleared failed-run journal \(reused=(\d+) recorded=(\d+)\)$', log, re.M)
    assert completion == [(str(len(reused)), str(len(recorded)))], 'CI completion counts disagree'
    return {'expected': sorted(phases), 'recorded': recorded, 'reused': reused}


def validate_previous(previous, head, command):
    assert previous['head'] == previous['head_after'] == head, 'prior gate head changed'
    assert previous['tracked_clean'] and previous['tracked_clean_after'], 'prior gate tracked inputs changed'
    assert previous['exit'] != 0 and not previous['qualified'], 'prior gate did not fail'
    assert previous['normal_priority'] is False, 'prior gate was not demoted'
    assert previous['argv'] == ['taskpolicy', '-b', 'nice', '-n', '15', *command], 'prior command/baseline differs'
    # The caller still reviews the preserved failure log to establish that
    # the sole observed failure was the existing performance ceiling.


def validate_priority(receipt, command):
    normal = receipt['normal_priority']
    assert type(normal) is bool, 'invalid priority record'
    prefix = [] if normal else ['taskpolicy', '-b', 'nice', '-n', '15']
    assert receipt['argv'] == [*prefix, *command], 'qualifying command/priority differs'
    if normal:
        assert receipt['previous_receipt'], 'normal priority has no prior failure record'
    else:
        assert receipt['previous_receipt'] is None, 'demoted invocation has unexpected prior record'
    return normal


def read_committed_walk_record(tree, certificate, read_committed):
    relative = Path(certificate).resolve().relative_to(Path(tree).resolve())
    name = relative.as_posix()
    assert name.startswith('docs/design/greenfield/slices/emitter-final-batch/integration/records/'), 'walk certificate is outside archived records'
    assert relative.name == 'certificate.json', 'unexpected walk certificate name'
    data = Path(certificate).read_bytes()
    assert data == read_committed(name), 'walk certificate differs from validation commit'
    return data


def profile_roster(paths):
    profiles = sorted(path for path in paths
                      if re.fullmatch(r'ratchets/h2-[0-9]+[a-z]-profile\.v1\.json', path))
    all_profiles = sorted(path for path in paths
                          if re.fullmatch(r'ratchets/h2-.+-profile\.v1\.json', path))
    assert profiles == all_profiles, 'unrecognized current H2 profile name'
    assert len(profiles) == len(set(profiles)) == 22, 'current H2 profile roster changed'
    roster = ['ratchets/h1-emit-profile.v1.json', *profiles, 'ratchets/h2-profile-transition.v1.json']
    assert set(roster) <= set(paths), 'required historical/transition profile absent'
    return roster


def validate_owner_count(rows):
    assert len(rows) == 18 and sum(map(len, rows.values())) == 74, 'architecture row/owner count changed'
