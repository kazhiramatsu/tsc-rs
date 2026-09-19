"""Prepared serial replay driver. Run only after census and the current native pipeline finish."""
from pathlib import Path
import subprocess, json, os
code = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-corpus-replay')
common_target = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target')
snapshot = common_target/'emitter-recovery-census-r78/parse-snapshot.json'
assert snapshot.is_file(), 'census snapshot is not complete'
for pid in (56011, 79147, 83365):
    try: os.kill(pid, 0)
    except ProcessLookupError: pass
    else: raise SystemExit(f'wait for active census/native pipeline {pid}')
head = subprocess.check_output(['git','-C',str(code),'rev-parse','HEAD'],text=True).strip()
assert head == '33fddc898e1086dad95830e3a9ef99fe816dadef', 'review replay driver if probe code changed'
assert not subprocess.check_output(['git','-C',str(code),'diff','HEAD','--name-only']).strip()
output = common_target/'emitter-recovery-parser-replays-r131'
output.mkdir(exist_ok=False)
steps = [
 ('current','candidate','/Users/hiramatsu/dev/tsc-rs-emitter-final-census',True),
 ('projection','projection','/Users/hiramatsu/dev/tsc-rs-emitter-final-parser-baseline',True),
 ('merge-base','merge-base','/Users/hiramatsu/dev/tsc-rs-emitter-final-parser-main',False),
 ('successor','successor','/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next',True),
]
for label,kind,tree,profiles in steps:
    assert Path(tree).is_dir(), tree
    args=['python3',str(code/'scripts/replay-recovery-parse.py'),'--parser-tree',tree,
          '--input',str(snapshot),'--out',str(output/(label+'.json')),
          '--build-dir',str(output/('build-'+label)), '--label','r131-'+label,'--baseline-kind',kind]
    if profiles: args.append('--with-recovery-profiles')
    print(json.dumps({'step':label,'argv':args}),flush=True)
    subprocess.run(args,cwd=code,check=True)
print('Four parser replays completed; source/row selection and native command qualification still required.',flush=True)
