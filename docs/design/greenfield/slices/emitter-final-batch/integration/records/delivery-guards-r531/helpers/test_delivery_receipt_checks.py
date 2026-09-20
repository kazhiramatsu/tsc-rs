import copy
from pathlib import Path
import subprocess
import tempfile
import sys
import unittest

from delivery_receipt_checks import (expected_phases, validate_phase_log,
                                     validate_previous, profile_roster,
                                     validate_owner_count)
from delivery_receipt_checks import validate_priority, read_committed_walk_record

ROOT = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
HEAD = 'bb01b86df0a2d266910d25124c4bfbade334c826'
MAIN = subprocess.check_output(['git', 'show', HEAD + ':crates/xtask/src/main.rs'], cwd=ROOT, text=True)
PATHS = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', HEAD, '--', 'ratchets'], cwd=ROOT, text=True).splitlines()
PHASES = sorted(expected_phases(MAIN))
COMMAND = ['cargo', 'xtask', 'ci', '--baseline', '3b1f5fe87fd31e3b303bb44bd257342735452ed9']


def log_for(recorded, reused):
    lines = []
    for phase in recorded:
        lines += ['local CI phase: run ' + phase, 'local CI checkpoint: recorded ' + phase]
    lines += ['local CI resume: reuse ' + phase + ' (exact inputs and outputs)' for phase in reused]
    lines += [f'local CI resume: complete; cleared failed-run journal (reused={len(reused)} recorded={len(recorded)})']
    return '\n'.join(lines) + '\n'


class ReceiptChecks(unittest.TestCase):
    def test_real_driver_refuses_stray_previous_before_any_gate(self):
        driver = Path(__file__).with_name('emitter-final-ci-run-r530.py')
        result = subprocess.run([sys.executable, str(driver), '--head', HEAD,
                                 '--label', 'invalid-previous-unit-control',
                                 '--previous', '/not-a-real-prior-receipt.json'],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('--previous requires --normal-priority', result.stderr)

    def test_full_and_resumed_logs_cover_actual_eighteen_phases(self):
        self.assertEqual(len(PHASES), 18)
        for split in (0, 7, 18):
            result = validate_phase_log(MAIN, log_for(PHASES[:split], PHASES[split:]))
            self.assertEqual(result['expected'], PHASES)

    def test_partial_duplicate_wrong_phase_and_completion_refuse(self):
        original = log_for(PHASES, [])
        mutants = [
            log_for(PHASES[:-1], []),
            log_for(PHASES[:-1] + ['unexpected-phase'], []),
            log_for(PHASES, [PHASES[0]]),
            original.replace('local CI phase: run ' + PHASES[0] + '\n', ''),
            original + 'local CI phase: run unexpected-phase\n',
            original.replace('recorded=18)', 'recorded=17)'),
            original.rsplit('\n', 2)[0] + '\n',
            original + original.splitlines()[-1] + '\n',
        ]
        for log in mutants:
            with self.subTest(log=log), self.assertRaises(AssertionError):
                validate_phase_log(MAIN, log)

    def test_non_utf8_unrelated_output_is_decoded_consistently(self):
        raw = log_for(PHASES, []).encode()
        self.assertEqual(validate_phase_log(MAIN, b'compiler output: \xff\n' + raw)['expected'], PHASES)
        corrupted = raw.replace(PHASES[0].encode(), b'\xff', 1)
        with self.assertRaises(AssertionError):
            validate_phase_log(MAIN, corrupted)

    def test_priority_and_previous_receipt_are_explicit(self):
        for normal in (False, True):
            receipt = {'normal_priority': normal, 'previous_receipt': 'prior.json' if normal else None,
                       'argv': ([] if normal else ['taskpolicy', '-b', 'nice', '-n', '15']) + COMMAND}
            self.assertEqual(validate_priority(receipt, COMMAND), normal)
            for key, value in [('normal_priority', 'false'), ('argv', ['unexpected', *COMMAND]),
                               ('previous_receipt', None if normal else 'unexpected.json')]:
                changed = copy.deepcopy(receipt)
                changed[key] = value
                with self.subTest(key=key, normal=normal), self.assertRaises(AssertionError):
                    validate_priority(changed, COMMAND)

    def test_walk_certificate_is_identical_to_archived_validation_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            tree = Path(directory) / 'tree'
            name = 'docs/design/greenfield/slices/emitter-final-batch/integration/records/walk/certificate.json'
            certificate = tree / name
            certificate.parent.mkdir(parents=True)
            certificate.write_bytes(b'certificate bytes')
            def committed(path):
                self.assertEqual(path, name)
                return b'certificate bytes'
            self.assertEqual(read_committed_walk_record(tree, certificate, committed), b'certificate bytes')
            certificate.write_bytes(b'altered bytes')
            with self.assertRaises(AssertionError):
                read_committed_walk_record(tree, certificate, committed)
            outside = Path(directory) / 'certificate.json'
            outside.write_bytes(b'certificate bytes')
            with self.assertRaises(ValueError):
                read_committed_walk_record(tree, outside, committed)
            certificate.unlink()
            certificate.symlink_to(outside)
            with self.assertRaises(ValueError):
                read_committed_walk_record(tree, certificate, committed)

    def test_prior_resume_requires_identical_failed_demoted_command_and_tree(self):
        previous = {'head': HEAD, 'head_after': HEAD, 'tracked_clean': True,
                    'tracked_clean_after': True, 'exit': 101, 'qualified': False,
                    'normal_priority': False,
                    'argv': ['taskpolicy', '-b', 'nice', '-n', '15', *COMMAND]}
        validate_previous(previous, HEAD, COMMAND)
        for key, value in [('head', 'changed'), ('head_after', 'changed'),
                           ('tracked_clean', False), ('tracked_clean_after', False),
                           ('exit', 0), ('qualified', True), ('normal_priority', True),
                           ('argv', COMMAND), ('argv', previous['argv'][:-1] + ['origin/main'])]:
            mutant = copy.deepcopy(previous)
            mutant[key] = value
            with self.subTest(key=key, value=value), self.assertRaises(AssertionError):
                validate_previous(mutant, HEAD, COMMAND)

    def test_profile_roster_comes_from_full_validation_tree(self):
        roster = profile_roster(PATHS)
        self.assertEqual(len(roster), 24)
        profile = roster[1]
        for paths in ([p for p in PATHS if p != profile], PATHS + [profile],
                      PATHS + ['ratchets/h2-9z-profile.v1.json'],
                      PATHS + ['ratchets/h2-7de-profile.v1.json'],
                      [p for p in PATHS if p != roster[0]],
                      [p for p in PATHS if p != roster[-1]]):
            with self.subTest(paths=len(paths)), self.assertRaises(AssertionError):
                profile_roster(paths)

    def test_owner_denominators_refuse_drift(self):
        rows = {str(i): [None] * (5 if i < 2 else 4) for i in range(18)}
        validate_owner_count(rows)
        for mutation in ('row', 'owner'):
            changed = copy.deepcopy(rows)
            if mutation == 'row':
                changed.pop('0')
            else:
                changed['0'].pop()
            with self.subTest(mutation=mutation), self.assertRaises(AssertionError):
                validate_owner_count(changed)


if __name__ == '__main__':
    unittest.main()
