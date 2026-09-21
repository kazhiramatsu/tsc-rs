from pathlib import Path
import re,json,hashlib
root=Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-recovery-next');problems=[];count=0;cache={}
for p in sorted((root/'crates').rglob('*.rs')):
 docs=[];start=0
 for number,line in enumerate(p.read_text().splitlines(),1):
  t=line.lstrip()
  if t.startswith('///'):
   if not docs:start=number
   docs.append(t[3:].strip());continue
  if not t or t.startswith('#['):continue
  if re.match(r'(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:const\s+)?fn\s+',t):
   ports=[i for i,d in enumerate(docs) if d.startswith('tsc-port:')]
   for n,i in enumerate(ports):
    count+=1;block=docs[i:ports[n+1] if n+1<len(ports) else len(docs)]
    port=block[0].split(':',1)[1].strip();parts=port.split();reasons=[]
    if len(parts)<2 or not parts[1].startswith('@'):reasons.append('malformed-port')
    digest=next((d.split(':',1)[1].strip().split()[0] for d in block if d.startswith('tsc-hash:')),None)
    span=next((d.split(':',1)[1].strip() for d in block if d.startswith('tsc-span:')),None)
    if digest is None:reasons.append('missing-hash')
    m=re.fullmatch(r'(.+):(\d+)-(\d+)',span or '')
    actual=None
    if not m:reasons.append('missing-or-malformed-span')
    else:
     file,a,b=m.groups();a=int(a);b=int(b)
     candidates=[root/'vendor/typescript-6.0.3/src/compiler'/file,root/'vendor/typescript-6.0.3/lib'/file,root/'ts-tests/src/compiler'/file,root.parent/'ts-tests/src/compiler'/file,root.parent/file,root/file]
     source=next((c for c in candidates if c.is_file()),None)
     if source is None:reasons.append('missing-source')
     else:
      if source not in cache:cache[source]=source.read_bytes().splitlines(keepends=True)
      lines=cache[source]
      if not 0<a<=b<=len(lines):reasons.append('invalid-span')
      else:
       actual=hashlib.sha256(b''.join(lines[a-1:b])).hexdigest()
       if digest and actual!=digest:reasons.append('stale-hash')
    if reasons:problems.append({'path':str(p.relative_to(root)),'line':start+i,'port':port,'span':span,'recorded':digest,'actual':actual,'reasons':reasons})
  docs=[]
report={'head':__import__('subprocess').check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(),'entries_scanned':count,'problems':problems}
Path('/tmp/emitter-ledger-r107.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
