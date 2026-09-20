from pathlib import Path
import hashlib,json,re
r=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep');o=Path('/tmp/emitter-l0-anchor-repair-r511')
p=r/'crates/oracle/h2-7a-owner-inventory.mjs';s=p.read_text()
rows=json.loads((o/'owner-header-candidates.json').read_text());lineage=json.loads((o/'owner-multiple-header-lineage.json').read_text());by_line={x['script_line']:x for x in lineage}
rx=re.compile(r'\b(m4Anchor|auditAlreadyExact)\(\s*"(crates/[^"\n]+):([1-9][0-9]*)"(?:\s*,\s*("(?:[^"\\]|\\.)*"))?\s*\)')
matches=list(rx.finditer(s));assert len(matches)==len(rows)==354
edits=[];choices=[]
for m,row in zip(matches,rows):
 assert m.start()==row['start']
 if row['header'] is None:assert row['already_valid'];continue
 if row['already_valid']:continue
 if len(row['candidates'])==1:
  selected=row['candidates'][0];reason='sole matching upstream header in the named Rust source';evidence=None
 else:
  evidence=by_line[row['script_line']]
  if len(evidence['same_fn_candidates'])==1:
   selected=evidence['same_fn_candidates'][0]['header_line'];reason='same original Rust owner, recovered from the commit introducing this recorded anchor'
  else:
   assert row['header']=='emitSignatureAndBody' and evidence['old_fn']['name']=='emit_transformed_node_worker'
   selected=2864;reason='original worker body now delegated by emit_transformed_node_worker to emit_transformed_node_worker_unscoped; preserve primary full-body header, not the Indented sub-branch'
 assert selected in row['candidates']
 edits.append((m.start(3),m.end(3),str(selected)))
 choices.append({'path':row['path'],'header':row['header'],'old_line':row['line'],'new_line':selected,'script_line':row['script_line'],'reason':reason,'lineage':evidence})
start=s.index('const H2_7B_ANCHORS =');end=s.index('\nfunction m4Anchor(',start);block=s[start:end]
name_rx=re.compile(r'path: "([^"\n]+)",\s*name: "([^"\n]+)",\s*line: (\d+)')
named=[]
for m in name_rx.finditer(block):
 path,name,old=m.groups();text=(r/path).read_text();fn=re.compile(r'^\s*(?:(?:pub(?:\([^)]*\))?|const|async|unsafe)\s+)*fn\s+'+re.escape(name)+r'\b',re.M);found=list(fn.finditer(text));assert len(found)==1,(path,name,len(found))
 # The regex may include preceding blank whitespace; locate the actual fn token line.
 line=text[:found[0].end()].count('\n')+1
 assert int(old)!=line
 edits.append((start+m.start(3),start+m.end(3),str(line)))
 named.append({'path':path,'name':name,'old_line':int(old),'new_line':line})
assert len(choices)==238 and len(named)==7 and len(edits)==245
candidate=s
for a,b,value in sorted(edits,reverse=True):candidate=candidate[:a]+value+candidate[b:]
(o/'h2-7a-owner-inventory.candidate.mjs').write_text(candidate)
sha=lambda b:hashlib.sha256(b).hexdigest()
paths={x['path'] for x in rows}|{x['path'] for x in named}
proof={'source_before_sha256':sha(s.encode()),'candidate_sha256':sha(candidate.encode()),'unchanged_headered':20,'unchanged_unheadered_range_only':96,'headered_changes':choices,'named_changes':named,'total_numeric_leaf_changes':len(edits),'current_rust_inputs':[{'path':path,'sha256':sha((r/path).read_bytes())} for path in sorted(paths)],'qualification':False,'scope':'Prepared line references only; no product or generator execution and no artifact mint. Original owner lineage retained for multi-header cases.'}
(o/'owner-anchor-lines-proposal.json').write_text(json.dumps(proof,indent=2)+'\n')
print(json.dumps({'numeric_leaf_changes':len(edits),'headered':len(choices),'named':named,'candidate':str(o/'h2-7a-owner-inventory.candidate.mjs')}))
