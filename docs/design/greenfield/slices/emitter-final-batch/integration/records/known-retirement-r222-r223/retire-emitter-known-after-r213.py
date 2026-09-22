"""Prepare retirement only after final controls/corpus/regression qualification."""
from pathlib import Path
import argparse,copy,hashlib,json,re,subprocess
p=argparse.ArgumentParser();p.add_argument('--tree',type=Path,required=True);p.add_argument('--out',type=Path,required=True);a=p.parse_args();r=a.tree.resolve();t=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final/target');b=Path('docs/design/greenfield/slices/emitter-final-batch/integration')
head=Path('/tmp/emitter-corpus-controls-r211-head').read_text().strip()
n=json.loads((t/'emitter-variable-type-r213-native/manifest.json').read_text());c=json.loads((t/'emitter-corpus-candidate-r212/manifest.json').read_text());controls=json.loads((t/'emitter-corpus-controls-r211/manifest.json').read_text())
assert n['head']==c['head']==controls['head']==head
assert c['qualified'] and all(s['exit']==0 for s in c['steps'])
assert controls['qualified'] and all(s['exit']==0 for s in controls['steps'])
steps={s['label']:s for s in n['steps']}
for name in ['variable-type-native-r213','variable-comma-native-r213','variable-type-neighbours-r213','jsdoc-original-r213','checker-lib-r213']:assert steps[name]['exit']==0
assert steps['variable-type-native-r213']['expected_cases']==508 and steps['variable-comma-native-r213']['expected_cases']==145
assert len([s for s in n['steps'] if s['label'].startswith('variable-emitter-') and s['exit']==0])==23
assert all(s['exit']==0 for s in n['steps'] if s['label']!='variable-type-transpile-r213')
assert steps['variable-type-transpile-r213']['exit']==101
checker_log=(Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')/b/'records/local/checker-lib-r213.log').read_text()
assert re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;.*?(\d+) filtered out;',checker_log)==[('ok','1739','0','0','0')]
reports=[json.loads((t/f'emitter-variable-type-r213-native/transpile-evidence/native-{route}.json').read_text()) for route in ['transpile-js','transpile-dts','program-no-check']]
assert all(not d['unexpected_open'] for d in reports)
retired={i for d in reports for i in d['unexpected_exact']};assert len(retired)==6 and sum(d['known_open'] for d in reports)==2 and sum(d['exact']+len(d['unexpected_exact']) for d in reports)==289
# Reject missing or partial command capture sets as well as red Rust tests.
for label,count in [('variable-type-native-r213',508),('variable-comma-native-r213',145),('variable-type-neighbours-r213',120)]:
 groups={}
 for path in (t/(label+'-captures')).glob('*.json'):
  row=json.loads(path.read_text());groups.setdefault(row['case_id'],[]).append(row)
 assert len(groups)==count,(label,len(groups),count)
 assert all(len(rows)==2 and all(row['error'] is None and row['actual']==row['expected'] for row in rows) for rows in groups.values()),label
# The independent JSDoc comparator asserts both runs in memory, without the capture hook.
jsdoc=json.loads((r/'crates/compiler/tests/fixtures/emitter-jsdoc-original-command.json').read_text())
assert len(jsdoc['cases'])==1 and jsdoc['repetitions']==2 and jsdoc['complete_command_executions']==2
selection=json.loads((t/'emitter-keyword-successor-r185/selection.json').read_text())
assert subprocess.check_output(['git','-C',str(r),'rev-parse','HEAD:crates/syntax'],text=True).strip()==selection['successor']['syntax_tree_hash']
assert selection['summary']['loaded_rows']==14219 and selection['summary']['selected_rows']==48 and selection['summary']['load_failures']==110
for path in subprocess.check_output(['git','-C',str(r),'diff','--name-only',head,'--','crates'],text=True).splitlines():raise AssertionError('qualification source changed: '+path)
assert not subprocess.check_output(['git','-C',str(r),'diff','HEAD','--name-only']).strip()
a.out.mkdir(parents=True,exist_ok=False);changes={};proof={'source_head':n['head'],'corpus_head':c['head'],'retired_transpile':sorted(retired),'files':[]}
def update(path,data):
 path=str(path);before=(r/path).read_bytes();data=data.encode() if isinstance(data,str) else data;out=a.out/path;out.parent.mkdir(parents=True,exist_ok=True);out.write_bytes(data);changes[path]=data;proof['files'].append({'path':path,'before_sha256':hashlib.sha256(before).hexdigest(),'after_sha256':hashlib.sha256(data).hexdigest()})
def dump(d):return json.dumps(d,indent=2,ensure_ascii=False)+'\n'
fixture='crates/compiler/tests/fixtures/emitter-final-known-native.json';d=json.loads((r/fixture).read_bytes());assert d==json.loads((r/b/'records/parse-known-before-retirement-r122.json').read_bytes());assert len(d['cases'])==36;d['cases']=[];update(fixture,dump(d))
path='crates/compiler/tests/emitter_final_universe.rs';s=(r/path).read_text()
for name,count in [('KNOWN',1),('KNOWN_PLAN_BASE',35)]:
 pattern=rf'(const {name}: &\[\(&str, &str\)\] = &)\[(.*?)\];';m=re.search(pattern,s,re.S);assert m and len(re.findall(r'^    \(',m[2],re.M))==count;s=s[:m.start()]+m[1]+'[];'+s[m.end():]
start=s.index('fn known_refusal_rejects_changed_error_or_partial_writes()');prefix=s[:start];tail=s[start:];old='include_str!("fixtures/emitter-final-known-native.json")';assert tail.count(old)==1;tail=tail.replace(old,'include_str!("../../../'+str(b)+'/records/parse-known-before-retirement-r122.json")');update(path,prefix+tail)
archive=b/'records/transpile-known-before-retirement-r161';live=Path('crates/compiler/tests/fixtures/h2_8c_transpile');k=json.loads((r/live/'known-open.v1.json').read_bytes());v=json.loads((r/live/'known-native.v1.json').read_bytes());assert k==json.loads((r/archive/'known-open.v1.json').read_bytes()) and v==json.loads((r/archive/'known-native.v1.json').read_bytes());assert len(k['rows'])==len(v['observations'])==8;remaining={row['id'] for row in k['rows']}-retired;assert remaining=={'h2-8c/transpile-js/text/unicode','h2-8c/transpile-js/numeric-target/transform-100'};k['rows']=[row for row in k['rows'] if row['id'] in remaining];v['observations']={i:row for i,row in v['observations'].items() if i in remaining};v['case_count']=2;update(live/'known-open.v1.json',dump(k));update(live/'known-native.v1.json',dump(v))
path='crates/compiler/tests/transpile_routes_contract.rs';s=(r/path).read_text();assert s.count('known["rows"].as_array().unwrap().len(), 8')==1 and s.count('known.len(), 8, "duplicate known-open ID"')==1;s=s.replace('known["rows"].as_array().unwrap().len(), 8','known["rows"].as_array().unwrap().len(), 2').replace('known.len(), 8, "duplicate known-open ID"','known.len(), 2, "duplicate known-open ID"');start=s.index('fn known_open_rejects_changed_output_refusal_and_panic()');end=s.index('\n#[test]',start);part=s[start:end];assert part.count('serde_json::from_str(KNOWN_NATIVE)')==1;part=part.replace('serde_json::from_str(KNOWN_NATIVE)','serde_json::from_str(include_str!("../../../'+str(archive)+'/known-native.v1.json"))');s=s[:start]+part+s[end:];update(path,s)
path='scripts/emitter_final_witnesses.py';s=(r/path).read_text();needle='PACKET + "integration/records/retired-checker-known.v1.json",';assert s.count(needle)==1;s=s.replace(needle,needle+'\n                PACKET + "integration/records/parse-known-before-retirement-r122.json",');update(path,s)
path='scripts/witness.py';s=(r/path).read_text();needle='for name in ("expected", "known-open", "known-native", "review-expected")),';assert s.count(needle)==1;s=s.replace(needle,'for name in ("expected", "known-open", "known-native", "review-expected"))\n                  + tuple("'+str(archive)+'/" + name + ".v1.json"\n                          for name in ("known-open", "known-native")),');update(path,s)
(a.out/'proposal.json').write_text(dump(proof));print(json.dumps({'proposal':str(a.out),'files':len(changes),'retired_universe':36,'retired_transpile':6,'remaining_transpile':2}));print('Proposal only. Review, copy into candidate, format and run the live guard/universe/transpile suites.')
