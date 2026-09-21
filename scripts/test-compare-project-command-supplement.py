import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('project_comparison', Path(__file__).with_name('compare-project-command-supplement.py'))
module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)

class ProjectCommandComparisonTests(unittest.TestCase):
    def artifacts(self):
        command_input = {'roots': ['/main.ts'], 'source': 'export const x=1;'}
        input_sha = hashlib.sha256(json.dumps(command_input, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
        command = {'writes': [{'path': '/main.js', 'callback': 'var x=1;', 'data_present': False}], 'reported_diagnostics': [], 'status_writes': [], 'exit_code': 0, 'emit_result': {'emit_skipped': False, 'diagnostics': [], 'emitted_files': None, 'source_maps': None}}
        common = {'schema': 1, 'head': 'source', 'input_head': 'data', 'roster_sha256': 'roster', 'compiler_sha256': 'ts', 'library_root': '/lib', 'input_workspace': '/data', 'repetitions': 2}
        case = {'case_id': 'project', 'input_sha256': input_sha, 'options': {'sourceMap': False}, 'complete_command_runs': [copy.deepcopy(command), copy.deepcopy(command)]}
        native = {**common, 'kind': 'emitter-project-projection-native', 'case_ids': ['project'], 'cases': [{**copy.deepcopy(case), 'row': {'case_id': 'project', 'loader': 'load_project_emit', 'command_input': command_input}}]}
        oracle = {**common, 'kind': 'emitter-project-projection-typescript', 'native_sha256': 'native', 'cases': [copy.deepcopy(case)]}
        return native, oracle

    def test_complete_command_difference_is_not_hidden(self):
        n, o = self.artifacts()
        self.assertEqual(module.compare(n, 'native', o)['summary']['dispositions'], {'complete-command-exact': 1})
        for field, changed in [('exit_code', 2), ('reported_diagnostics', [{'code': 1110}]), ('status_writes', ['status']), ('writes', []), ('emit_result', {'emit_skipped': True})]:
            n, o = self.artifacts()
            for run in o['cases'][0]['complete_command_runs']: run[field] = changed
            self.assertEqual(module.compare(n, 'native', o)['cases'][0]['disposition'], 'complete-command-mismatch; emit-not-qualified', field)

    def test_options_and_refusals_cannot_qualify(self):
        n, o = self.artifacts(); o['cases'][0]['options']['sourceMap'] = True
        self.assertEqual(module.compare(n, 'native', o)['cases'][0]['disposition'], 'input-option-mismatch; emit-not-qualified')
        n, o = self.artifacts(); n['cases'][0]['complete_command_runs'] = [{'production_error': 'refused', 'partial_writes': []}] * 2
        self.assertEqual(module.compare(n, 'native', o)['cases'][0]['disposition'], 'production-refusal; emit-not-qualified')

    def test_coverage_identity_and_repetition_guards(self):
        for mutate in [lambda n,o: o.update(native_sha256='wrong'), lambda n,o: o['cases'].clear(), lambda n,o: o['cases'].append(copy.deepcopy(o['cases'][0])), lambda n,o: o['cases'][0].update(input_sha256='wrong'), lambda n,o: o['cases'][0]['complete_command_runs'][1].update(exit_code=1)]:
            n, o = self.artifacts(); mutate(n,o)
            with self.assertRaises(AssertionError): module.compare(n, 'native', o)

    def test_roster_pin_and_observation_shape(self):
        n, o = self.artifacts()
        raw = json.dumps({'schema': 1, 'kind': 'emitter-project-projection-roster', 'case_ids': n['case_ids']}).encode()
        n['roster_sha256'] = hashlib.sha256(raw).hexdigest()
        module.validate_roster(n, raw)
        with self.assertRaises(AssertionError): module.validate_roster(n, raw + b' ')
        n, o = self.artifacts(); del o['cases'][0]['options']
        self.assertEqual(module.compare(n, 'native', o)['cases'][0]['disposition'], 'command-observation-invalid; emit-not-qualified')
        n, o = self.artifacts()
        for artifact in (n, o): artifact['cases'][0]['complete_command_runs'] = [{}, {}]
        self.assertEqual(module.compare(n, 'native', o)['cases'][0]['disposition'], 'command-observation-invalid; emit-not-qualified')
        n['cases'] = [{'case_id': 'project', 'disposition': 'not-loaded; emit-not-qualified', 'load_error': 'bad root'}]
        with self.assertRaises(AssertionError): module.compare(n, 'native', o)

    def test_load_failure_remains_unqualified(self):
        n, o = self.artifacts()
        n['cases'] = [{'case_id': 'project', 'disposition': 'not-loaded; emit-not-qualified', 'load_error': 'bad root'}]
        o['cases'] = [{'case_id': 'project', 'disposition': 'native-input-unavailable; emit-not-qualified'}]
        self.assertEqual(module.compare(n, 'native', o)['summary']['dispositions'], {'not-loaded; emit-not-qualified': 1})

if __name__ == '__main__': unittest.main()
