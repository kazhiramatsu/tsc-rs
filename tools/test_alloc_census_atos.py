"""Offline symbolization contract, including image relocation and PC-4."""
import importlib.util
import json
from pathlib import Path
from types import SimpleNamespace
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('alloc_census_atos', Path(__file__).with_name('alloc_census_atos.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class AtosTests(unittest.TestCase):
    def test_exact_return_pc_map_uses_recorded_load_and_call_instructions(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sidecar'
            path.write_text(json.dumps(dict(unwinder='darwin-arm64-frame-pointer-v1', load_address='0x100000000')) + '\n' +
                            json.dumps(dict(pcs=['0x100001004', '0x100001004', '0x180001004'])) + '\n')
            def run(args, **kwargs):
                self.assertEqual(args[:7], ['atos', '-arch', 'arm64', '-o', 'binary', '-l', '0x100000000'])
                self.assertEqual(Path(args[8]).read_text(), '0x100001000\n0x180001000\n')
                self.assertTrue(kwargs['check'])
                return SimpleNamespace(stdout='producer (in binary)\n0x180001000\n')
            self.assertEqual(m.symbolize(path, 'binary', run), dict(exact=True, symbols={
                '0x100001004': 'producer (in binary)', '0x180001004': '0x180001004'}))

    def test_rust_v0_names_are_demangled_offline_without_changing_pc_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sidecar'
            path.write_text(json.dumps(dict(unwinder='darwin-arm64-frame-pointer-v1', load_address='0x1000')) + '\n' +
                            json.dumps(dict(pcs=['0x1004'])) + '\n')
            def run(args, **kwargs):
                if args[0] == 'atos':
                    return SimpleNamespace(stdout='_RNvNtExample (in binary) + 8\n')
                self.assertEqual(args, ['xcrun', 'llvm-cxxfilt', '--no-strip-underscore'])
                self.assertEqual(kwargs['input'], '_RNvNtExample (in binary) + 8\n')
                return SimpleNamespace(stdout='tsc_program::producer (in binary) + 8\n')
            self.assertEqual(m.symbolize(path, 'binary', run)['symbols']['0x1004'],
                             'tsc_program::producer (in binary) + 8')

    def test_rejects_missing_metadata_empty_pcs_and_inconsistent_atos_output(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'sidecar'
            for header, pcs in [(dict(unwinder='unsupported'), ['0x1004']),
                                (dict(unwinder='darwin-arm64-frame-pointer-v1', load_address='0x0'), ['0x1004']),
                                (dict(unwinder='darwin-arm64-frame-pointer-v1', load_address='0x1000'), [])]:
                path.write_text(json.dumps(header) + '\n' + json.dumps(dict(pcs=pcs)) + '\n')
                with self.assertRaises(ValueError):
                    m.symbolize(path, 'binary')
            path.write_text(json.dumps(dict(unwinder='darwin-arm64-frame-pointer-v1', load_address='0x1000')) + '\n' +
                            json.dumps(dict(pcs=['0x1004'])) + '\n')
            with self.assertRaises(ValueError):
                m.symbolize(path, 'binary', lambda *a, **k: SimpleNamespace(stdout=''))


if __name__ == '__main__':
    unittest.main()
