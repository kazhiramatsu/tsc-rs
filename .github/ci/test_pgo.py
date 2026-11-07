#!/usr/bin/env python3
"""Offline PGO orchestration tests: all toolchain/compiler processes are fakes."""
import base64
import contextlib
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, HERE/f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


pgo = load('pgo')
measurement = load('pgo_measure')


class RecipeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.out = self.root/'out'
        (self.out/'logs').mkdir(parents=True)
        (self.out/'profiles').mkdir()
        self.corpus = self.root/'corpora'
        self.corpus.mkdir()

    def corpus_fixture(self):
        for name in ['hono', 'zod']:
            directory = self.corpus/name
            directory.mkdir()
            (directory/'tsconfig.ci-pgo.json').write_text('{}')
        batches = self.corpus/'conformance'
        batches.mkdir()
        for listing in (ROOT/'.github/pgo').glob('*.cases'):
            (batches/listing.name).write_bytes(listing.read_bytes())
        pgo.write_json(self.corpus/'manifest.json', dict(recipe='release', files=pgo.inventory(self.corpus)))

    def fake_list(self, *args, **kwargs):
        cases = sorted({line for p in (ROOT/'.github/pgo').glob('*.cases')
                        for line in p.read_text().splitlines()})
        return '\n'.join(cases+[f'compiler/filler{i}.ts' for i in range(12000)])

    def fake_training(self, command, **kwargs):
        self.assertNotIn('cargo', command)
        self.assertEqual(kwargs['env']['TSRS_WORKERS'], '1')
        self.assertEqual(kwargs['env']['TSRS_CHECKERS'], '1')
        raw = Path(kwargs['env']['LLVM_PROFILE_FILE'].replace('%p', '123').replace('%m', 'abc'))
        raw.write_bytes(b'profile')
        return subprocess.CompletedProcess(command, 2 if '-p' in command else 0)

    def test_exactly_two_fixed_batches_and_two_projects(self):
        self.corpus_fixture()
        with patch.object(pgo.subprocess, 'check_output', side_effect=self.fake_list), \
             patch.object(pgo.subprocess, 'run', side_effect=self.fake_training) as runner, \
             contextlib.redirect_stdout(io.StringIO()):
            pgo.train(ROOT, self.out, self.corpus, 'release')
        self.assertEqual(runner.call_count, 4)
        training = json.loads((self.out/'logs/training.json').read_text())
        self.assertEqual(training['projects'], ['hono', 'zod'])
        self.assertEqual(len(training['batches']), 2)

    def test_missing_profile_fails_even_with_successful_exit(self):
        with patch.object(pgo.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0)):
            with self.assertRaisesRegex(RuntimeError, 'missing/empty training profile'):
                pgo.training_run(['fake-cli'], ROOT, self.out, 'missing', 1, project=True)

    def test_experiment_keeps_full_conformance_suites_and_typescript(self):
        self.corpus_fixture()
        directory = self.corpus/'typescript/src/compiler'
        directory.mkdir(parents=True)
        (directory/'tsconfig.ci-pgo.json').write_text('{}')
        pgo.write_json(self.corpus/'manifest.json', dict(recipe='experiment', files=pgo.inventory(self.corpus)))
        with patch.object(pgo.subprocess, 'check_output', side_effect=self.fake_list), \
             patch.object(pgo.subprocess, 'run', side_effect=self.fake_training) as runner, \
             contextlib.redirect_stdout(io.StringIO()):
            pgo.train(ROOT, self.out, self.corpus, 'experiment')
        training = json.loads((self.out/'logs/training.json').read_text())
        selected = [line for listing in sorted((self.out/'logs').glob('*.cases'))
                    for line in listing.read_text().splitlines()]
        self.assertEqual(selected, sorted(self.fake_list().splitlines()))
        self.assertEqual(training['projects'], ['hono', 'typescript'])
        self.assertEqual(runner.call_count, len(training['batches'])+3)
        self.assertTrue(any('suites-ts71' in str(call.args[0][0]) for call in runner.call_args_list))

    def test_crash_fails_instead_of_silently_skipping_training(self):
        with patch.object(pgo.subprocess, 'run', return_value=subprocess.CompletedProcess([], -11)):
            with self.assertRaisesRegex(RuntimeError, 'abnormal exit'):
                pgo.training_run(['fake-cli'], ROOT, self.out, 'crash', 1)

    def test_changed_corpus_fails_before_any_process(self):
        self.corpus_fixture()
        (self.corpus/'zod/tsconfig.ci-pgo.json').write_text('{"changed":true}')
        with patch.object(pgo.subprocess, 'run') as runner:
            with self.assertRaisesRegex(RuntimeError, 'frozen corpus bytes changed'):
                pgo.train(ROOT, self.out, self.corpus, 'release')
            runner.assert_not_called()

    def test_merge_excludes_build_profiles_and_removes_objects(self):
        (self.out/'profiles/build.profraw').write_bytes(b'build')
        training = self.out/'profiles/train-zod-123.profraw'
        training.write_bytes(b'training')
        (self.out/'instrumented').mkdir()
        def fake(command, **kwargs):
            if 'merge' in command:
                self.assertEqual(command[-1], str(training))
                self.assertNotIn(str(self.out/'profiles/build.profraw'), command)
                (self.out/'merged.profdata').write_bytes(b'merged')
            return subprocess.CompletedProcess(command, 0)
        with patch.object(pgo.pgo_build, 'llvm_profdata', return_value='fake-profdata'), \
             patch.object(pgo.subprocess, 'run', side_effect=fake), \
             contextlib.redirect_stdout(io.StringIO()):
            pgo.merge(ROOT, self.out)
        self.assertFalse((self.out/'instrumented').exists())

    def test_locked_build_command_and_environment_without_executing_cargo(self):
        with patch.dict(os.environ, LLVM_PROFILE_FILE='poison', CARGO_ENCODED_RUSTFLAGS='poison'):
            def fake(command, **kwargs):
                self.assertIn('--locked', command)
                self.assertEqual(command.count('-p'), 2)
                self.assertNotIn('LLVM_PROFILE_FILE', kwargs['env'])
                self.assertNotIn('CARGO_ENCODED_RUSTFLAGS', kwargs['env'])
                self.assertEqual(kwargs['cwd'], ROOT)
                return subprocess.CompletedProcess(command, 0)
            pgo.pgo_build.cargo_build(self.out, '-Cprofile-generate=fake', 2,
                root=ROOT, locked=True, packages=('tsc-rs-cli', 'tsc-rs-conformance'), runner=fake)

    def test_package_hashes_provenance_and_executable_mode(self):
        self.corpus_fixture()
        (self.out/'tsc-rs').write_bytes(b'fake executable')
        (self.out/'tsc-rs').chmod(0o755)
        (self.out/'merged.profdata').write_bytes(b'merged')
        pgo.write_json(self.out/'logs/training.json', dict(recipe='release'))
        def output(command, **kwargs):
            if command[0] == 'rustc':
                return 'rustc 1.99.1 (test)\nhost: aarch64-apple-darwin\n'
            if command[0] == 'git':
                return 'd7c481a6e'+'0'*31+'\n'
            return 'LLVM test\n'
        for kind in ['normal', 'pgo']:
            dist = self.root/kind
            with patch.object(pgo.subprocess, 'check_output', side_effect=output), \
                 patch.object(pgo.pgo_build, 'llvm_profdata', return_value='fake-profdata'), \
                 patch.object(pgo.platform, 'platform', return_value='test-platform'):
                pgo.package(ROOT, self.out, self.corpus, kind, dist)
            archive = next(dist.glob('*.tar.gz'))
            with tarfile.open(archive) as tar:
                prefix = archive.name.removesuffix('.tar.gz')
                meta = json.load(tar.extractfile(prefix+'/BUILD.json'))
                self.assertEqual(meta['kind'], kind)
                self.assertEqual(meta['release_profile']['lto'], 'fat')
                self.assertEqual(meta['release_profile']['codegen_units'], 1)
                self.assertEqual(meta['profdata_sha256'], pgo.sha256(self.out/'merged.profdata') if kind == 'pgo' else None)
                self.assertTrue(tar.getmember(prefix+'/tsc-rs').mode & 0o111)
                for line in tar.extractfile(prefix+'/SHA256SUMS').read().decode().splitlines():
                    digest, name = line.split('  ')
                    self.assertEqual(hashlib.sha256(tar.extractfile(prefix+'/'+name).read()).hexdigest(), digest)

    def test_freeze_with_integrity_checked_offline_archives(self):
        # Exercise the downloaded-byte path without network or external tools.
        archives = {}
        packages = {}
        for name, version in [('@types/node', '25.3.3'), ('undici-types', '7.18.2')]:
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode='w:gz') as tar:
                entry = tarfile.TarInfo('package/index.d.ts')
                entry.size = 10
                tar.addfile(entry, io.BytesIO(b'export {};'))
            payload = data.getvalue()
            url = 'https://registry.npmjs.org/'+name
            archives[url] = payload
            packages['node_modules/'+name] = dict(version=version, resolved=url,
                integrity='sha512-'+base64.b64encode(hashlib.sha512(payload).digest()).decode())
        for path in ['hono', 'typescript/src/compiler', 'zod/packages/zod/src']:
            (self.corpus/path).mkdir(parents=True)
        pgo.write_json(self.corpus/'typescript/package-lock.json', dict(packages=packages))
        def git(command, **kwargs):
            name = Path(command[2]).name
            return pgo.PINS[name]+('0'*33 if name == 'zod' else '')+'\n'
        with patch.object(pgo.urllib.request, 'urlopen', side_effect=lambda url, **kw: io.BytesIO(archives[url])), \
             patch.object(pgo.subprocess, 'check_output', side_effect=git), \
             patch.object(pgo.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0)), \
             contextlib.redirect_stdout(io.StringIO()):
            pgo.freeze(self.corpus, 'release')
        manifest = pgo.verify_corpus(self.corpus)
        self.assertEqual(len(manifest['batches']), 2)
        self.assertEqual(len(manifest['zod']), 40)


class MeasurementTests(unittest.TestCase):
    def _exercise_measurement(self, failure=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root/'corpora/hono').mkdir(parents=True)
            (root/'corpora/manifest.json').write_text('{}')
            for side in ['normal', 'pgo']:
                binary = root/'binaries'/side
                binary.mkdir(parents=True)
                (binary/'tsc-rs').write_bytes(side.encode())
                pgo.write_json(binary/'BUILD.json', dict(kind=side, binary_sha256=pgo.sha256(binary/'tsc-rs'),
                    corpus_sha256=pgo.sha256(root/'corpora/manifest.json'), commit='same', rustc='same',
                    target='same', release_profile={}, recipe_sha256='same', helper_sha256='same',
                    cargo_lock_sha256='same', vendor_manifest_sha256='same', profdata_sha256='profile'))
            order = []
            def fake(command, **kwargs):
                binary = next(item for item in command if item.endswith('/tsc-rs'))
                side = Path(binary).parent.name
                order.append(side)
                kwargs['stdout'].write(b'changed\n' if failure == 'diagnostics' and side == 'pgo' else b'diagnostics\n')
                if command[0] == 'valgrind':
                    count = 1000 if side == 'normal' else 800
                    if failure == 'unstable' and len(order) == 20:
                        count += 1
                    Path(next(c.split('=', 1)[1] for c in command if c.startswith('--cachegrind-out-file='))).write_text(f'events: Ir\nsummary: {count}\n')
                    Path(next(c.split('=', 1)[1] for c in command if c.startswith('--log-file='))).write_text(f'I refs: {count:,}\n')
                else:
                    Path(command[3]).write_text('Maximum resident set size (kbytes): 1024\n')
                return subprocess.CompletedProcess(command, 2)
            with patch.object(measurement.subprocess, 'run', side_effect=fake), \
                 patch.object(measurement.subprocess, 'check_output', return_value='valgrind fake'), \
                 contextlib.redirect_stdout(io.StringIO()):
                summary = measurement.measure(root, root/'results', 'hono', release=True)
            self.assertEqual(order, ['normal', 'pgo']*10)
            self.assertEqual(summary['sides']['normal']['ir_samples'], [1000, 1000])
            self.assertEqual(summary['sides']['pgo']['ir'], 800)
            self.assertEqual(len(json.loads((root/'results/samples.json').read_text())), 20)

    def test_interleaving_raw_counts_and_output_identity(self):
        self._exercise_measurement()

    def test_changed_diagnostics_reject_comparison(self):
        with self.assertRaisesRegex(RuntimeError, 'diagnostic/exit identity changed'):
            self._exercise_measurement('diagnostics')

    def test_unstable_repeated_counts_reject_comparison(self):
        with self.assertRaisesRegex(SystemExit, 'Repeated I refs differ'):
            self._exercise_measurement('unstable')


if __name__ == '__main__':
    unittest.main()
