"""Reobserve these inputs with the pinned tsgo executable supplied as argv[1]."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

root = Path(__file__).resolve().parent
binary = Path(sys.argv[1]).resolve()
manifest = json.loads((root / 'cases.json').read_text())
for case in manifest['cases']:
    directory = root / case['name']
    for name, text in case['files'].items():
        assert (directory / name).read_text() == text, (case['name'], name)
    assert json.loads((directory / 'tsconfig.json').read_text()) == case['config']
    case['case_sensitive'] = not (directory / 'GLOBALS.D.TS').exists()
    for mode, observation in case['observations'].items():
        run = subprocess.run([str(binary)] + observation['command'], cwd=directory,
                             capture_output=True, check=False)
        assert run.returncode in (0, 2), (case['name'], mode, run.returncode)
        prefix = str(directory).encode()
        (directory / f'{mode}.stdout.txt').write_bytes(run.stdout.replace(prefix, b'<fixture>'))
        (directory / f'{mode}.stderr.txt').write_bytes(run.stderr.replace(prefix, b'<fixture>'))
        observation['rc'] = run.returncode
        if mode == 'showConfig':
            case['expected_roots'] = json.loads(run.stdout)['files']
manifest['binary_sha256'] = hashlib.sha256(binary.read_bytes()).hexdigest()
(root / 'cases.json').write_text(json.dumps(manifest, indent=2, ensure_ascii=True) + '\n')
