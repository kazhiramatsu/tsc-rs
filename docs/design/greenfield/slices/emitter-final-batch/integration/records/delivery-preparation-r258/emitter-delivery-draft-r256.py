"""Prepare one post-merge Markdown draft from actual final receipts; no git mutation."""
import argparse, datetime, gzip, hashlib, json, os, re, subprocess
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
for name in ['tree','input-audit','ci-receipt','hosted-receipt','out']:p.add_argument('--'+name,type=Path,required=True)
a=p.parse_args()
audit=json.loads(a.input_audit.read_bytes());ci=json.loads(a.ci_receipt.read_bytes());hosted=json.loads(a.hosted_receipt.read_bytes())
v=audit['validation_ref'];m=audit['merge_ref'];baseline='3b1f5fe87fd31e3b303bb44bd257342735452ed9'
def git(*args):return subprocess.check_output(['git','-C',str(a.tree),*args])
def show(path):return git('show',f'{v}:{path}')
def sha(data):return hashlib.sha256(data).hexdigest()
assert git('rev-parse','HEAD').decode().strip()==m
assert not git('diff','HEAD','--name-only').strip()
subprocess.run(['git','-C',str(a.tree),'merge-base','--is-ancestor',v,m],check=True)
assert git('rev-parse',v+'^{tree}')==git('rev-parse',m+'^{tree}')
assert ci['head']==v and ci['tracked_clean'] and ci['exit']==0
argv=ci['argv'];at=argv.index('cargo')
assert argv[at:]==['cargo','xtask','ci','--baseline',baseline]
log=gzip.decompress(a.ci_receipt.with_suffix('.log.gz').read_bytes())
assert sha(log)==ci['log_sha256'] and len(log)==ci['log_bytes']
assert hosted['headRefOid']==v and hosted['state']=='MERGED' and hosted['mergeCommit']['oid']==m
checks=hosted['statusCheckRollup'];assert checks
for check in checks:
 if check.get('__typename')=='StatusContext':assert check['state']=='SUCCESS',check
 else:
  assert check.get('status')=='COMPLETED' and check.get('conclusion') in ['SUCCESS','SKIPPED','NEUTRAL'],check
assert audit['owner_identity_evidence'].startswith('docs/')
assert len(audit['profiles'])==24 and len(audit['selected_rows'])==18
assert not any(x['stale_runtime_inputs'] for x in audit['profiles'])
assert next(x for x in audit['profiles'] if x['path']=='ratchets/h2-5g-profile.v1.json')['runtime_input_count']==921
base=Path('docs/design/greenfield/slices/emitter-final-batch/integration')
record=base/'architecture-validation.md'
def rel(path):return os.path.relpath(path,base)
proof=json.loads(show(str(base/'records/delivery-preparation-r251/emitter-delivery-proof-matrix-r250.json')))
assert set(proof['rows'])==set(audit['selected_rows'])
for profile in audit['profiles']:assert sha(show(profile['path']))==profile['sha256']
source=show(audit['typescript_source']);assert sha(source)==audit['typescript_source_sha256']
units=source.decode().encode('utf-16-le')
for owner in [x for owners in audit['selected_rows'].values() for x in owners]+[audit['adjacent_checker_owner']]:
 for key,hashkey in [('source_range','declaration_sha256'),('body_range','body_sha256')]:
  span=owner[key];data=units[span['start']['offset']*2:span['end']['offset']*2].decode('utf-16-le').encode()
  assert sha(data)==owner[hashkey]
end=datetime.datetime.fromisoformat(ci['started_at'])+datetime.timedelta(seconds=ci['seconds'])
date=end.date().isoformat()
text=f'''# Emitter-final architecture validation

This record owns the lifecycle and validation fields delegated by the 18 rows
below in the [current architecture map](../../../emitter-architecture.md).
Their invariants, Rust owners and boundaries remain in that map. Qualification
is limited to the scopes and tests recorded here.

Final validation ref: `{v}`. Validation date: {date} UTC.
Actual merge ref: `{m}` ([PR #561](https://github.com/kazhiramatsu/tsc-rs/pull/561)).
The merge contains the validation ref, and both tracked trees equal
`{audit['validation_tree']}`. Thus every tracked runtime, test and evidence
input is byte-identical on delivery. The documentation commit carrying this
record is neither the validation ref nor a runtime/profile input.

The clean validation ref passed the complete, unsplit local command
`cargo xtask ci --baseline {baseline}` and the required hosted checks.
The local raw-log SHA-256 is `{ci['log_sha256']}`;
its receipt SHA-256 is `{sha(a.ci_receipt.read_bytes())}`.
The completed gate summary and hosted result are recorded in PR #561.
The [integration report](README.md) retains separately attributed focused
proofs and prior failures; they do not replace this final-ref gate.

The original 68 producer-known rows are retired. The separate transpile set
retains two bounded known outcomes: composite recovery for an invalid Unicode
identifier and internal JSON target 100. This qualification does not extend
to all H2.9 recovery shapes, build/watch or builder state, public API re-emit,
or TypeScript 7. The parser census's 110 load failures remain unqualified;
the 108 separately executed projects are not substitutes for those inputs.

## Profile and delivery manifest

The 22 H2 runtime profiles and H2 transition below were checked through the
canonical converged walk and final gate. Their complete `runtime_inputs`
path/hash arrays are bound by each linked artifact's SHA-256; every current
runtime-input hash was independently compared with the validation ref.
H2.5g contains exactly 921 registered runtime inputs. These hashes bind source
identity and do not establish execution coverage by themselves. The H1 emit
profile is retained as historical lineage; its internal historical inputs are
not asserted to describe the current runtime tree.

| Artifact at final validation ref | SHA-256 | Scope |
| --- | --- | --- |
'''
for row in audit['profiles']:
 text+=f"| [{row['path']}]({rel(row['path'])}) | `{row['sha256']}` | {row['classification']} |\n"
text+=f'''
## TypeScript owner identity

The owner identities below are primary routing owners with scoped branch
proofs, not an exhaustive call-graph closure. All are extracted from
[TypeScript 6.0.3 `_tsc.js`]({rel(audit['typescript_source'])}), SHA-256
`{audit['typescript_source_sha256']}`. Declaration and body slices use UTF-16
source offsets; their SHA-256 values were recomputed from the final validation
ref. The [extracted identity record]({rel(audit['owner_identity_evidence'])})
retains exact declaration/body spans and lexical scopes.

The adjacent checker correction belongs to
`{audit['adjacent_checker_owner']['lexical_path']}`
(body SHA-256 `{audit['adjacent_checker_owner']['body_sha256']}`).
A missing parsed function body infers `any`; a real empty body still infers
`void`. It crosses the existing typed resolver boundary and adds no
architecture owner. Its scoped proof includes the declaration/map commands,
the strict no-false-TS2322 control, checker regressions and final conformance.
'''
for row,owners in audit['selected_rows'].items():
 entry=proof['rows'][row]
 text+=f"\n## {row}\n\nLifecycle: `active-qualified`.\n\nFinal validation ref: `{v}`. Validation date: {date} UTC.\n\nBounded scope: {entry['scope']}\n\nScoped source/test references, exercised through the corresponding final-gate\ntargets and the separately attributed focused proofs:\n\n"
 for path in entry['test_references']:
  show(path)
  text+=f'- [{path}]({rel(path)})\n'
 text+='\nTypeScript routing owners (full body SHA-256):\n\n'
 for owner in owners:
  text+=f"- `{owner['lexical_path']}`: `{owner['body_sha256']}`.\n"
 if row=='E-RECOVERY-FACTS':
  text+='\nThe retained parser event/action representation, rollback and reparse rules\nremain parser-owned. The admitted class-member gap requires the one retained\nreport, one reachable gap, a preceding function-like member with a parsed\nzero-width block, and the next member or list boundary. Unrelated actions use\nthe existing context solver. The five predecessor profiles are not widened;\n`E-SYNTAX-FACTS` keeps its separate planned lifecycle. The original 16,994-input\nparser proof and complete newly admitted command comparisons retain their own\nsource attribution.\n'
 if row in ['E-COMMENTS-G','E-COMMENT-PHASES-A36','E-COMMENT-ELLIPSIS-A37']:
  text+='\nThe [prior lifecycle audit](records/architecture-comments-before-r201.json)\npreserves the earlier qualification. Current Unicode collection, comment\nphases and map ordering are covered only within the enumerated controls.\n'
assert text.count('Lifecycle: `active-qualified`.')==18
assert 'pending' not in text.lower()
with a.out.open('x') as f:f.write(text)
print(json.dumps({'draft':str(a.out),'validation_ref':v,'merge_ref':m,'rows':18,'changed_record_only':str(record),'note':'Draft only: review rendered diff, relative links/anchors and unchanged README STATUS before landing Markdown-only D.'}))
