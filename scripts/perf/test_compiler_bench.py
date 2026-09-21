"""Exercise measurement integrity with tiny fake processes, never real compilers."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).with_name('compiler_bench.py')
spec = importlib.util.spec_from_file_location('compiler_bench', SCRIPT)
bench = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bench)
REPO = SCRIPT.resolve().parents[2]


class MeasurementTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.suite = bench.prepare(self.root / 'suite')

    def compiler(self, name, fault=''):
        path = self.root / name
        path.write_text(f'#!{sys.executable}\n' + '''
import json, sys, time
from pathlib import Path
FAULT = ''' + repr(fault) + '''
if '--version' in sys.argv:
    print('FAKE compiler used only to test measurement plumbing')
    sys.exit(0)
config = Path(sys.argv[sys.argv.index('-p') + 1])
output = Path(sys.argv[sys.argv.index('--outDir') + 1])
if config.parent.name == 'control':
    if FAULT == 'noop': sys.exit(0)
    print('index.ts(1,7): error TS2322: Type string is not assignable to type number.')
    sys.exit(1)
if FAULT == 'fail': sys.exit(3)
if FAULT == 'timeout': time.sleep(10)
if FAULT == 'diagnostics': print('unexpected diagnostics')
if FAULT == 'mutate': (config.parent / 'index.ts').write_text('changed')
if '--noEmit' not in sys.argv or FAULT == 'unexpected-write':
    suffixes = ['.js']
    if '--declaration' in sys.argv: suffixes += ['.d.ts', '.d.ts.map', '.js.map']
    for source in json.loads(config.read_text())['files']:
        for suffix in suffixes:
            (output / (Path(source).stem + suffix)).write_text('different' if FAULT == 'bytes' else 'same')
''')
        path.chmod(0o755)
        return path

    def compare(self, fault='', mode='emit', timeout='5'):
        output = self.root / 'results'
        result = subprocess.run([
            sys.executable, str(SCRIPT), 'run', '--suite', str(self.suite),
            '--rust', str(self.compiler('rust')), '--tsgo', str(self.compiler('native', fault)),
            '--rust-source', str(REPO), '--tsgo-source', str(REPO),
            '--output', str(output), '--cases', 'startup', '--modes', mode,
            '--repeats', '2', '--timeout', timeout,
        ], capture_output=True, text=True, timeout=30)
        self.assertTrue((output / 'results.json').exists(), result.stderr)
        return result, json.loads((output / 'results.json').read_text())

    def test_balanced_order_warmup_exclusion_and_per_child_resources(self):
        result, report = self.compare(mode='full-emit')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(report['status'], 'complete')
        measured = [r for r in report['runs'] if r['phase'] == 'measured']
        self.assertEqual([r['compiler'] for r in measured], ['tsgo', 'rust', 'rust', 'tsgo'])
        self.assertEqual(report['comparisons'][0]['samples_per_compiler'], 2)
        self.assertTrue(all(r['peak_rss_bytes'] > 0 and r['wall_seconds'] > 0 for r in measured))
        self.assertTrue(all(len(r['outputs']) == 4 for r in measured))
        self.assertFalse(report['performance_qualification'])

    def test_invalid_observations_never_produce_speed_ratios(self):
        for fault, mode in [('bytes', 'emit'), ('fail', 'emit'), ('noop', 'check'),
                            ('diagnostics', 'check'), ('unexpected-write', 'check'), ('mutate', 'emit')]:
            with self.subTest(fault=fault):
                # Each experiment owns new inputs/results so mutation tests remain isolated.
                with tempfile.TemporaryDirectory() as directory:
                    self.root = Path(directory)
                    self.suite = bench.prepare(self.root / 'suite')
                    result, report = self.compare(fault, mode)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertEqual(report['status'], 'invalid')
                    self.assertEqual(report['comparisons'], [])
                    self.assertTrue(report['runs'])

    def test_stale_inputs_are_rejected_before_compilation(self):
        (self.suite.parent / 'startup/index.ts').write_text('changed')
        result, report = self.compare()
        self.assertEqual(result.returncode, 1)
        self.assertIn('Input mutation', report['error'])
        self.assertEqual(report['runs'], [])

    def test_timeout_is_retained_and_child_is_reaped(self):
        row = bench.measure([sys.executable, '-c', 'import time; time.sleep(10)'],
                            self.root, os.environ.copy(), self.root / 'timeout', 0.1)
        self.assertTrue(row['timeout'])
        self.assertLess(row['exit'], 0)
        self.assertLess(row['wall_seconds'], 3)

    def test_prepare_does_not_overwrite_existing_inputs(self):
        original = self.suite.read_bytes()
        with self.assertRaises(FileExistsError):
            bench.prepare(self.suite.parent)
        self.assertEqual(original, self.suite.read_bytes())

    def test_child_rss_does_not_inherit_previous_child_maximum(self):
        large = bench.measure([sys.executable, '-c',
                               'a=bytearray(128*1024*1024); a[::4096]=b"x"*len(a[::4096])'],
                              self.root, os.environ.copy(), self.root / 'large', 10)
        small = bench.measure([sys.executable, '-c', 'pass'], self.root,
                              os.environ.copy(), self.root / 'small', 10)
        self.assertEqual(large['exit'], 0)
        self.assertEqual(small['exit'], 0)
        self.assertGreater(large['peak_rss_bytes'] - small['peak_rss_bytes'], 64 * 1024 * 1024)

    def test_allocator_and_gc_environment_does_not_leak(self):
        variables = {'GODEBUG': 'gctrace=1', 'MALLOC_CONF': 'junk:true',
                     'MALLOC_ARENA_MAX': '1', 'MallocNanoZone': '0', 'GOMAXPROCS': '20'}
        with mock.patch.dict(os.environ, variables):
            env = bench.environment({'GOMAXPROCS': '1'})
            self.assertTrue(set(variables).issubset(bench.removed_environment_keys()))
            self.assertTrue(all(key not in env for key in variables if key != 'GOMAXPROCS'))
            self.assertEqual(env['GOMAXPROCS'], '1')
            self.assertEqual(os.environ['GOMAXPROCS'], '20')

    @unittest.skipUnless(sys.platform == 'darwin', 'Darwin background policy')
    def test_taskpolicy_background_is_refused_even_with_nice_zero(self):
        code = ('import os,sys; sys.path.insert(0,sys.argv[1]); import compiler_bench; '
                'print(os.getpriority(os.PRIO_PROCESS,0), flush=True); compiler_bench.priority()')
        result = subprocess.run(['taskpolicy', '-b', sys.executable, '-c', code, str(SCRIPT.parent)],
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.stdout.strip(), '0')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Darwin background policy is active', result.stderr)


if __name__ == '__main__':
    unittest.main()
