import hashlib, sys, re
F='vendor/typescript-6.0.3/lib/_tsc.js'
lines=open(F,'rb').read().split(b'\n')
def end_of(start):
    depth=0; seen=False
    for i in range(start-1, len(lines)):
        for ch in lines[i]:
            if ch==ord('{'): depth+=1; seen=True
            elif ch==ord('}'): depth-=1
        if seen and depth==0: return i+1
    raise SystemExit('unbalanced')
def h(a,b): return hashlib.sha256(b'\n'.join(lines[a-1:b])+b'\n').hexdigest()
def find(name, after=0, indent=None):
    pat=re.compile(rb'^(\s*)(?:async )?function '+name.encode()+rb'\(')
    for i in range(after, len(lines)):
        m=pat.match(lines[i])
        if m and (indent is None or len(m.group(1))==indent): return i+1
    raise SystemExit('missing '+name)
specs=[l.strip().split() for l in sys.stdin if l.strip()]
for spec in specs:
    name=spec[0]; after=int(spec[1]) if len(spec)>1 else 0
    if name.isdigit():
        a=int(name); b=int(spec[1]); print(f'{spec[2]}\t{a}-{b}\t{h(a,b)}'); continue
    a=find(name, after); b=end_of(a); print(f'{name}\t{a}-{b}\t{h(a,b)}')
