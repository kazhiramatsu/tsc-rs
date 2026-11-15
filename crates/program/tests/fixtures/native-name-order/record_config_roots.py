"""Record native root order for the frozen CFG1b input set; retain its JS oracle."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

root = Path(__file__).resolve().parent
binary = Path(sys.argv[1]).resolve()
inputs_path = root.parent / 'h2-8b-config-discovery-inputs.json'
oracle_path = root.parent / 'h2-8b-config-discovery.json'
inputs = json.loads(inputs_path.read_text())
old = {c['case_id']: c['typescript_observation']
       for c in json.loads(oracle_path.read_text())['cases']}
rows = []
with tempfile.TemporaryDirectory(prefix='native-config-roots-') as temporary:
    for case in inputs['cases']:
        case_root = (Path(temporary) / case['case_id'].split('/')[-1]).resolve()
        project = case_root / 'project'
        replacement = str(project)
        def materialize(name, text):
            assert name.startswith('/project/')
            path = case_root / name.lstrip('/')
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text.replace('/project', replacement))
            return path
        for file in case['files']:
            materialize(file['path'], file['text'])
        config = materialize(case['config_path'], case['config'])
        command = [str(binary), '-p', str(config), '--showConfig', '--pretty', 'false']
        run = subprocess.run(command, cwd=config.parent, capture_output=True)
        if run.returncode == 0:
            values = json.loads(run.stdout).get('files', [])
            files = [os.path.normpath(str(config.parent / name)).replace(replacement, '/project', 1)
                     for name in values]
        else:
            assert run.returncode == 1 and b'error TS18003' in run.stdout, (case['case_id'], run.returncode, run.stdout)
            files = []
        assert sorted(files) == sorted(old[case['case_id']]['file_names']), (case['case_id'], files)
        rows.append(dict(case_id=case['case_id'], file_names=files, rc=run.returncode,
                         command=[part.replace(str(case_root), '<case>') for part in command],
                         stdout=run.stdout.decode().replace(str(case_root), '<case>'),
                         stderr=run.stderr.decode().replace(str(case_root), '<case>'),
                         case_sensitive=not (project / 'app/MAIN.TS').exists()))
manifest = dict(reference='pinned tsgo 7.1 native root order',
                binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                inputs_sha256=hashlib.sha256(inputs_path.read_bytes()).hexdigest(),
                javascript_oracle_sha256=hashlib.sha256(oracle_path.read_bytes()).hexdigest(), cases=rows)
(root / 'config-discovery-roots.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=True) + '\n')
print(len(rows), 'native CFG1b root lists recorded; frozen JS input/oracle unchanged')
