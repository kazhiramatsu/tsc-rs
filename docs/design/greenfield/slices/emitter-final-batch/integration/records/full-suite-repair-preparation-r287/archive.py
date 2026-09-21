from pathlib import Path
import gzip,hashlib,json,subprocess
ROOT=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
BASE=ROOT/'docs/design/greenfield/slices/emitter-final-batch/integration'
HEAD='628562c31a5102348a7e8769f3a2aa88deef70f1'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()==HEAD
assert not subprocess.check_output(['git','diff','HEAD','--name-only'],cwd=ROOT).strip()
assert not subprocess.check_output(['git','status','--porcelain','--','crates'],cwd=ROOT).strip()
assert (BASE/'records/prewalk-validation-r244').is_dir(), 'archive completed r244 first'
manifest=json.loads(Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-prewalk-r244/manifest.json').read_text())
assert len(manifest['steps'])==7, 'all r244 stages must finish first'
OUT=BASE/'records/full-suite-repair-preparation-r287'
items=[]
def add(source,destination,scope):
 source=Path(source); assert source.is_file(), str(source)
 data=source.read_bytes();items.append((source,destination,scope,data))
for number in [169,170,171,172,173,174,175,176]:
 add(f'/tmp/emitter-claude-review-round{number}-opus.json',f'reviews/round{number}-opus.json','actual read-only Claude review; includes model/turn metadata, not runtime qualification')
 add(f'/tmp/emitter-round{number}-request.md',f'reviews/round{number}-request.md','actual read-only review request')
for label,scope,names in [
 ('emitter-stale-declaration-controls-r264','unapplied3stale declaration control corrections; actual169review, product untouched',['proposal.patch.gz','proposal.json']),
 ('emitter-import-owner-proposal-r273','REJECTED first caller projection proposal; lost clipping/error semantics',['proposal.patch.gz','proposal.json','controls.patch.gz','controls.json']),
 ('emitter-import-owner-proposal-r279','unapplied final import clause modifier-erasure fix and36controls; runtime pending',['proposal.patch.gz','proposal.json','controls.patch.gz','controls.json','observe-token-comment-phases.mjs']),
 ('emitter-leading-binding-prototype-r281','superseded first composite admission proposal after172review; never applied',['proposal.patch.gz','proposal.json','tests-append.rs']),
 ('emitter-leading-binding-prototype-r283','unapplied revised bounded syntax prototype; no build/real-command/corpus qualification',['proposal.patch.gz','proposal.json','tests-append.rs','STATUS.md','round173-test-wiring-correction.json']),
 ('emitter-import-refusal-retirement-r282','unapplied12stale-refusal retirement; source command completes but fullcomparison pending',['proposal.patch.gz','proposal.json','observe-import-helpers.mjs']),
]:
 for name in names:add(Path('/tmp')/label/name,f'proposals/{label}/{name}',scope)
for name in ['emitter-current-failures-r271.json','emitter-recovery-inspection-r272.rs','emitter-recovery-inspection-r272.ts','emitter-recovery-inspection-r272.log','emitter-recovery-inspection-r272.json','emitter-recovery-family-design-r278.md','emitter-native-boundary-inventory-r284.json','emitter-remote-state-r285.json','emitter-repair-membership-audit-r286.json']:
 add(Path('/tmp')/name,f'inspection/{name}','read-only interim inspection/preparation; r244 full archived logs own final failure list')
for number in [274,275,276,277]:
 stem=f'emitter-recovery-output-exploration-r{number}'
 for suffix in ['.rs','-build.json','-build.log']:
  add(Path('/tmp')/(stem+suffix),'exploration/'+stem+suffix,'MOCK resolver; deliberately cleared recovery via existing harness hook; not qualification. r274build failed, r275/r276incomplete mock, r277fourJSbodies only')
 if number>274:
  for suffix in ['.json','.log']:add(Path('/tmp')/(stem+suffix),'exploration/'+stem+suffix,'same MOCK exploration limits; no realchecker/declarations/maps/status/exit qualification')
for suffix in ['.rs','-build.json','-build.log']:
 add(Path('/tmp')/('emitter-leading-binding-baseline-r289'+suffix),'inspection/emitter-leading-binding-baseline-r289'+suffix,'read-only baseline syntax/ambient/source-position inspection, not prototype execution')
for source in sorted(Path('/tmp/emitter-leading-binding-baseline-r289-records').iterdir()):
 if source.is_file():add(source,'inspection/baseline-r289/'+source.name,'same read-only baseline inspection; every ES5 input still refused at628, ES2015 clean')
for folder in ['emitter-leading-binding-controls-r290','emitter-leading-binding-tests-baseline-r291','emitter-leading-binding-tests-r292']:
 for source in sorted((Path('/tmp')/folder).iterdir()):
  if source.is_file():add(source,'inspection/'+folder+'/'+source.name,'unapplied controls and read-only baseline; r290 standalone-let positive superseded by r292 exported-let positive and standalone-let negative; production283 unchanged')
for number in [293,294,295]:
 add(Path('/tmp')/f'emitter-leading-binding-runtime-r{number}.py',f'syntax-feasibility/r{number}/runner.py','isolated syntax feasibility only, canonical unchanged; r293fmt failed,r294test compilation failed,r295passed')
 for source in sorted((Path('/tmp')/f'emitter-leading-binding-runtime-r{number}').iterdir()):
  if source.is_file():add(source,f'syntax-feasibility/r{number}/'+source.name,'same isolated syntax-only scope, not complete compiler compatibility')
for source in sorted(Path('/tmp/emitter-leading-binding-syntax-r296').iterdir()):
 if source.is_file():add(source,'syntax-feasibility/r296/'+source.name,'isolated2df8d1cd0 syntax213passed including42recovery; canonical628unchanged, fullcommands/corpus pending')
for folder in ['emitter-import-owner-consumers-r297','emitter-leading-binding-proof-inspection-r298','emitter-leading-binding-proof-r299','emitter-node-option-diagnostics-r300','emitter-node-option-diagnostics-r301','emitter-node-option-control-r302']:
 for source in sorted((Path('/tmp')/folder).iterdir()):
  if source.is_file() and source.name!='inspect':add(source,'followups/'+folder+'/'+source.name,'read-only/preparatory followups; r299preflight/10guards only, freshreplay notrun; r300libfingerprint mismatch thenr301native+TS inspection;302unapplied test correction')
add('/tmp/emitter-node-option-diagnostics-r300.rs','followups/emitter-node-option-diagnostics-r300.rs','read-only exactcurrentcontracts dependency fingerprint selected in301; initial300 buildfailed')
for folder in ['emitter-empty-variable-boundary-r303','emitter-empty-variable-neighbours-r305','emitter-empty-variable-ast-r306','emitter-historical-source-promotion-r307','emitter-historical-source-promotion-r308','emitter-json-owner-activity-r310','emitter-empty-variable-controls-r312']:
 for source in sorted((Path('/tmp')/folder).rglob('*')):
  if source.is_file() and source.name not in ['inspect','probe']:
   add(source,'late-audit/'+folder+'/'+str(source.relative_to(Path('/tmp')/folder)),'read-only audit or unapplied proposal;307 three original full commands exact twice;310 identifies activity expectations separately from output comparison; no canonical repair yet')
for name in ['emitter-empty-variable-ast-r306.rs','emitter-historical-source-promotion-r307.py','emitter-json-owner-activity-r310.py','emitter-postprewalk-r304.py','emitter-leading-binding-full-command-r309.py','emitter-empty-variable-flags-r311-test.rs','emitter-leading-binding-next-proofs-r314.py','emitter-full-suite-failure-classification-r313.json']:
 add(Path('/tmp')/name,'late-audit/'+name,'auditable local runner or read-only native probe source, no full qualification claim')
add('/Users/hiramatsu/dev/tsc-rs-emitter-final/target/emitter-postprewalk-r304/manifest.json','late-audit/postprewalk-r304-manifest.json','completed frozen-source archiving and ignored control status')
add(__file__,'archive.py','archival script, not a gate')
OUT.mkdir(exist_ok=False);index=[]
for source,destination,scope,data in items:
 path=OUT/destination;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(data)
 index.append({'path':destination,'source':str(source),'scope':scope,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
report={'head':HEAD,'qualified':False,'status':'preparation, failed/exploratory evidence and isolated syntax feasibility only; no proposal applied to canonical628 at archival time','r244_qualified':manifest['qualified'],'artifacts':index,'limits':['Actual r244 failure archive remains authoritative; interim271 has only failures known at that instant.','Old273projection rejected;281syntax superseded283. No proposal claims a fixed exact command.','Importhelpers12 commandOk does not yet prove full comparison.','r277 mocked JS body equality excludes maps/declarations/diagnostics and ordinary command semantics.','Opus173testwiring objection corrected by actual cfg(test) module path and Cargo metadata, not by product visibility change.','Prior original68retirement does not remove newly found active failures; actual final gates still required.']}
(OUT/'manifest.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({'path':str(OUT),'artifacts':len(index),'qualified':False}))
