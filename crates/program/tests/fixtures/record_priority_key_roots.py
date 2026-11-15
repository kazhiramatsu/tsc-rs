"""Reproduce priority-key-native-cases.json with the pinned binary as argv[1].

Inputs are the config and file names in each case. Non-JSON files contain
`export {};\n`; JSON source files contain `{}\n`. No compilation/build occurs.
The full CLI observations retain every diagnostic and exit status.
argv[2] is the output JSON; use a temporary output to verify reproducibility.
"""
from pathlib import Path
import hashlib,json,os,subprocess,sys,tempfile
fixture=Path(__file__).with_name('priority-key-native-cases.json')
manifest=json.loads(fixture.read_text());binary=Path(sys.argv[1]).resolve()
assert hashlib.sha256(binary.read_bytes()).hexdigest()==manifest['binary_sha256']
env={k:v for k,v in os.environ.items() if not k.startswith(('TSRS_','SERIAL_')) and k not in ['LLVM_PROFILE_FILE','DYLD_INSERT_LIBRARIES','GOMAXPROCS']}
with tempfile.TemporaryDirectory(prefix='priority-native-roots-') as temporary:
 for case in manifest['cases']:
  root=(Path(temporary)/case['name']).resolve();root.mkdir()
  (root/'tsconfig.json').write_text(json.dumps(case['config'],ensure_ascii=True)+'\n')
  for name in case['files']:
   path=root/name;path.parent.mkdir(parents=True,exist_ok=True)
   path.write_text('{}\n' if name.endswith('.json') else 'export {};\n')
  inputs={str(path.relative_to(root)):hashlib.sha256(path.read_bytes()).hexdigest() for path in sorted(root.rglob('*')) if path.is_file()}
  assert inputs==case['input_sha256'],case['name']
  assert (not (root/case['files'][0].upper()).exists())==case['case_sensitive']
  for row in case['observations']:
   command=[str(binary)]+row['command'][1:]
   run=subprocess.run(command,cwd=root,env=env|row['budget_environment'],capture_output=True)
   assert run.returncode in (0,1,2),(command,run.returncode,run.stderr)
   stdout=run.stdout.replace(str(root).encode(),b'<fixture>')
   stderr=run.stderr.replace(str(root).encode(),b'<fixture>')
   assert run.returncode==row['rc'] and stdout.decode()==row['stdout'] and stderr.decode()==row['stderr'],(case['name'],row['budget'],row['mode'])
   assert hashlib.sha256(stdout).hexdigest()==row['stdout_sha256']
   assert hashlib.sha256(stderr).hexdigest()==row['stderr_sha256']
   if row['mode']=='showConfig':assert json.loads(stdout)['files']==case['expected_roots']
Path(sys.argv[2]).write_text(json.dumps(manifest,indent=2,ensure_ascii=True)+'\n')
print('All18 complete native observations reproduced exactly')
