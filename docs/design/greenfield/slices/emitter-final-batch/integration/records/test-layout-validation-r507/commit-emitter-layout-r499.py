from pathlib import Path
import hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';D=Path('/tmp/emitter-layout-commit-r499');h=lambda b:hashlib.sha256(b).hexdigest()
def git(*a):return subprocess.check_output(['git',*a],cwd=R)
report=json.loads(Path('/tmp/emitter-layout-validation-r498/manifest.json').read_bytes());assert report['focused_qualified'];assert git('rev-parse','HEAD').decode().strip()==report['source_head']=='4ccf822b8b14555d6c07339e39dfced95327df33'
for path,digest in report['source_inputs'].items():assert h((R/path).read_bytes())==digest,path
subprocess.run(['python3','/tmp/archive-emitter-test-layout-r496.py'],cwd=R,check=True)
readme=B/'README.md';addition=Path('/tmp/emitter-layout-followup-r493/README-addition.md').read_text();assert 'records/test-layout-validation-r496/manifest.json' not in readme.read_text();anchor = "\n## walk事前検証の修復（r456–r485）";assert readme.read_text().count(anchor)==1;readme.write_text(readme.read_text().replace(anchor,addition+anchor,1))
subprocess.run(['git','diff','--check'],cwd=R,check=True)
changed=git('diff','--name-only','HEAD').decode().splitlines();new=git('ls-files','--others','--exclude-standard','--','crates','scripts').decode().splitlines()
assert len(new)==16,(len(new),new)
for path in changed:
 assert path in report['source_inputs'] or path=='crates/diagnostics/src/js_string/tests.rs' or path==str(readme.relative_to(R)),path
archive=str((B/'records/test-layout-validation-r496').relative_to(R));D.mkdir(exist_ok=False)
body='''emitter final: outline unit tests and bind the frozen census layout exception

Move 15 unit modules from 14 source files without changing production token
streams or fixture targets. Preserve the original census producer bytes with
one hash-bound layout exception and tested rejection of drift, disappearance,
and loss of workspace membership. Bind scanner and descriptor to preflight
receipt reuse. Update the two existing source hashes and classify 15 outlined
modules as tests; H2.5g runtime input set is 920 after removing one source test.

Validation: affected libraries 2,431 tests; command observer 26; qualification
50; workspace units 25; D/E units 2; native recovery units 6; actual workspace
audit and pin preflight. Preserve prior failures and actual Opus 203-205 review
in records/test-layout-validation-r496. Official walk and final unsplit CI
remain required before delivery.
'''
(D/'commit-message.txt').write_text(body)
subprocess.run(['git','add','--',*changed,*new,archive],cwd=R,check=True)
subprocess.run(['git','diff','--cached','--check'],cwd=R,check=True)
subprocess.run(['git','commit','-F',str(D/'commit-message.txt')],cwd=R,check=True)
assert not git('diff','HEAD','--name-only').strip();assert not git('ls-files','--others','--exclude-standard','--','crates','scripts').strip()
result=dict(head=git('rev-parse','HEAD').decode().strip(),source_qualified_focused=True,final_ci_qualified=False,archive=archive,changed_paths=changed,new_paths=new)
(D/'manifest.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
