from pathlib import Path
import datetime,gzip,hashlib,json,subprocess
R=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');B=R/'docs/design/greenfield/slices/emitter-final-batch/integration';O=B/'records/walk-preflight-repair-r485'
h=lambda b:hashlib.sha256(b).hexdigest()
def git(*a):return subprocess.check_output(['git',*a],cwd=R)
assert git('rev-parse','HEAD').decode().strip()=='2ee2bd6a96f0e8bd0605ffa4f6e59ffc9dbcad69'
assert not git('diff','HEAD','--','crates').strip()
O.mkdir(exist_ok=False);rows=[]
def put(name,b):
 p=O/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(b);rows.append(dict(path=name,sha256=h(b),bytes=len(b)))
def capture(p,name=None):
 p=Path(p);b=p.read_bytes();name=name or p.name
 if p.suffix=='.log':b=gzip.compress(b,mtime=0);name+='.gz'
 put(name,b)
for directory in ['emitter-walk-order-review-r457','emitter-walk-order-followup-r459','emitter-walk-frozen-plan-r461','emitter-frozen-reference-review-r463','emitter-frozen-reference-prototype-r464','emitter-frozen-reference-prototype-r465','emitter-frozen-reference-code-review-r468','emitter-frozen-current-projection-r470','emitter-walk-topology-review-r475','emitter-walk-topology-projection-r476','emitter-walk-complete-topology-review-r477','emitter-schema-order-repair-r481']:
 for p in sorted((Path('/tmp')/directory).iterdir()):
  if p.is_file():capture(p,directory+'/'+p.name)
for name in ['emitter-frozen-preflight-r466.log','emitter-frozen-tests-r467.log','emitter-frozen-replay-r469.log','emitter-frozen-pin-preflight-r471.log','emitter-frozen-qualification-tests-r473.log','emitter-frozen-qualification-tests-r479.log','emitter-frozen-hosted-selection-r480.json','emitter-frozen-qualification-tests-r482.log','chain-walk-fmt.log','chain-walk-inline-tests.log']:
 capture(Path('/tmp')/name)
for p in sorted(Path('/tmp/emitter-frozen-code-candidate-r472').rglob('*')):
 if p.is_file():capture(p,'first-replay-code-r472/'+str(p.relative_to('/tmp/emitter-frozen-code-candidate-r472')))
for label in ['final-walk-pin-preflight-r456','final-walk-dry-r456','walk-candidate-reference-check-r458','final-walk-dry-r474','final-walk-dry-r478']:
 p=B/'records/local'/f'{label}.json';d=json.loads(p.read_bytes());packed=p.with_suffix('.log.gz');assert h(gzip.decompress(packed.read_bytes()))==d['log_sha256'];capture(p,'local/'+p.name);capture(packed,'local/'+packed.name)
source=['scripts/check-frozen-de-reference.mjs','scripts/frozen-de-reference.test.mjs','scripts/chain-walk.sh','scripts/walk-topology-audit.py','new-ci/src/bin/plan.rs','.github/ci/qualification.mjs','.github/ci/qualification.test.mjs']
for name in source:capture(R/name,'final-source/'+name)
put('ci-only.patch.gz',gzip.compress(git('diff','HEAD'),mtime=0));capture(Path(__file__))
# Read-only verification of the snapshot's full closure (the original parents
# deliberately come from immutable archives, not from post-walk current bytes).
manifest=json.loads((R/'scripts/frozen-de-reference/manifest.json').read_bytes())
for row in manifest['files']:
 b=(R/row['path']).read_bytes()
 if 'snapshot' in row:
  z=(R/row['snapshot']['path']).read_bytes();assert h(z)==row['snapshot']['sha256'];b=gzip.decompress(z)
 assert len(b)==row['bytes'] and h(b)==row['sha256'],row['path']
qualification=Path('/tmp/emitter-frozen-qualification-tests-r482.log').read_text();assert 'pass 50' in qualification and 'fail 0' in qualification
result=dict(schema=1,scope='Integration preflight repair only; no product/crate change, no sanctioned real walk, no final qualification.',base_head='2ee2bd6a96f0e8bd0605ffa4f6e59ffc9dbcad69',created_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),retained_failures={'r456':'unregistered three reference generators','r464':'symlinked vendor changes TypeScript default library root; isolated prototype only','r474':'existing ORDER inversion exposed before any mint','r479':'schema registration order not yet synchronized; repaired r481 and all50 pass r482','r478':'registry/planner/topology/current projection/pins/static checks/fmt green; test-module layout16 locations refuses before Clippy/mint'},success={'r465':'copy-only original325 and323x2 reference checks','r467':'five mutation test groups green','r469':'candidate325, original observations323x2, bundle55x2, outputdirectory23x2; pre/postcurrentprojection green','r470':'current four-parent metadata-only drift passes; current owner drift refuses; restore passes','r480':'all five new CI/input path classes select full hosted3groups+78witnesses','r482':'all50 structural qualification tests green; 27schema mappings preserved with3entryreorder'},outstanding='Move16 existing cfg(test) modules to tests/unit and verify content equivalence, affected suites, source mirrors; then one real walk, full unsplit final CI, hosted gates, merge and final qualification.',files=rows)
(O/'manifest.json').write_text(json.dumps(result,indent=2)+'\n')
for row in rows:assert h((O/row['path']).read_bytes())==row['sha256']
print(json.dumps(dict(archive=str(O),files=len(rows),bytes=sum(r['bytes'] for r in rows))))
