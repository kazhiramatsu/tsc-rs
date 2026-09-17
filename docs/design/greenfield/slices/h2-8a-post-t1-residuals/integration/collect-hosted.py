#!/usr/bin/env python3
"""Collect the pinned PR #555 jobs; publish a receipt only after complete success."""
from pathlib import Path
import datetime,gzip,hashlib,json,re,subprocess,sys
ROOT=Path(__file__).resolve().parents[6]
OUT=ROOT/'docs/design/greenfield/slices/h2-8a-post-t1-residuals/integration/hosted'
OUT.mkdir(parents=True,exist_ok=True)
HEAD='2883e3c79247b988106c6e4f1f2bdd074ae90e66'
RUNS=[35185982007,35185981972]
REPO='kazhiramatsu/tsc-rs'
def gh(*args):
    return subprocess.check_output(['gh',*map(str,args)],cwd=ROOT)
def seconds(job):
    return int((datetime.datetime.fromisoformat(job['completedAt'].replace('Z','+00:00'))-datetime.datetime.fromisoformat(job['startedAt'].replace('Z','+00:00'))).total_seconds())
workflows=[]
for run in RUNS:
    data=json.loads(gh('run','view',run,'--json','databaseId,headSha,headBranch,event,status,conclusion,createdAt,updatedAt,url,jobs'))
    assert data['headSha']==HEAD and data['event']=='pull_request',data
    for job in data['jobs']:
        if job['status']!='completed': continue
        job['seconds']=seconds(job)
        path=OUT/(str(job['databaseId'])+'.log.gz')
        if not path.exists():
            result=subprocess.run(['gh','api',f'repos/{REPO}/actions/jobs/{job["databaseId"]}/logs'],cwd=ROOT,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
            if result.returncode:
                job['log_pending']=result.stderr.decode().strip()
                continue
            path.write_bytes(gzip.compress(result.stdout,mtime=0))
        raw=gzip.decompress(path.read_bytes())
        job.update(log=path.name,log_sha256=hashlib.sha256(path.read_bytes()).hexdigest(),log_uncompressed_sha256=hashlib.sha256(raw).hexdigest())
        checkouts=re.findall(r'git log -1 --format=%H\r?\n\S+ ([0-9a-f]{40})',raw.decode())
        assert len(checkouts)==1,(job['name'],checkouts)
        job['checkout_commit']=checkouts[0]
    workflows.append(data)
alljobs=[job for run in workflows for job in run['jobs']]
record={'version':1,'candidate_commit':HEAD,'candidate_tree':gh('api',f'repos/{REPO}/git/commits/{HEAD}','--jq','.tree.sha').decode().strip(),'pr':f'https://github.com/{REPO}/pull/555','workflows':workflows}
progress={'jobs':len(alljobs),'success':sum(j['conclusion']=='success' for j in alljobs),'failure':[(j['name'],j['conclusion']) for j in alljobs if j['conclusion'] not in ('','success')],'running':[j['name'] for j in alljobs if j['status']!='completed'],'logs':sum('log' in j for j in alljobs)}
Path('/tmp/post-t1-hosted-progress.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps(progress))
if all(run['status']=='completed' for run in workflows):
    assert len(alljobs)==14 and all(j['conclusion']=='success' and 'log' in j for j in alljobs),progress
    checkouts={job['checkout_commit'] for job in alljobs}
    assert len(checkouts)==1,checkouts
    checkout=next(iter(checkouts))
    checkout_commit=json.loads(gh('api',f'repos/{REPO}/git/commits/{checkout}'))
    assert checkout_commit['tree']['sha']==record['candidate_tree']
    checkout_parents=[p['sha'] for p in checkout_commit['parents']]
    assert checkout_parents==['3de6ab9bc7ecbee096699e927dd3e0f35f2c923e',HEAD],checkout_parents
    record['tested_checkout']={'commit':checkout,'tree':checkout_commit['tree']['sha'],'parents':checkout_parents,'same_tree_as_candidate':True}
    def log(name):
        job=next(j for j in alljobs if j['name']==name)
        return gzip.decompress((OUT/job['log']).read_bytes()).decode()
    pipeline=log('witnesses (decorator-binding-pipeline)')
    controls=log('witnesses (controls)')
    required=[('pipeline','decorator binding SUMMARY exact=767 known=0 failed=0 selected=767',pipeline),('post-t1','post t1 residuals SUMMARY exact=96 known=5 failed=0 selected=101',pipeline),('post-t1-packet','post t1 residuals PACKET SUMMARY exact=79 known=0 failed=0 probed=79',pipeline),('t1','bundle metadata t1 SUMMARY exact=18 known=0 failed=0 selected=18',controls),('t1-packet','bundle metadata t1 PACKET SUMMARY exact=15 known=0 failed=0 probed=15',controls)]
    for name,expected,text in required:
        assert expected in text,(name,expected)
    replay=[j for j in alljobs if j['name'].startswith(('acceptance (','witnesses ('))]
    assert len(replay)==10
    record.update(replay_jobs=10,replay_total_seconds=sum(j['seconds'] for j in replay),replay_longest_seconds=max(j['seconds'] for j in replay),observations={name:expected for name,expected,_ in required},timing_excludes=['plans','gates','main push'],workers=2,remaining_known={'complete_commands':5,'packet_probes':0,'owner':'E-COMMENT-SCOPE-H bound decorator target trailing comments'})
    observer_lines=[line for line in pipeline.splitlines() if '{"destination":' in line and 'post-t1-residuals.json' in line]
    assert len(observer_lines)==1
    observer=json.loads(observer_lines[0][observer_lines[0].index('{'):])
    assert observer['cases']==101
    post_t1_tail=pipeline[pipeline.index(observer_lines[0]):]
    rust_times=re.findall(r'test result: ok\. 2 passed;.*finished in ([0-9.]+)s',post_t1_tail)
    assert len(rust_times)==1
    record['post_t1_timing']={'observer_seconds':observer['elapsed_seconds'],'rust_test_seconds':float(rust_times[0]),'excludes':['Cargo build','checkout and setup'],'shared_compiler_build':True}
    (OUT/'receipt.v1.json').write_text(json.dumps(record,indent=2)+'\n')
    print('FINAL_RECEIPT_VERIFIED')
