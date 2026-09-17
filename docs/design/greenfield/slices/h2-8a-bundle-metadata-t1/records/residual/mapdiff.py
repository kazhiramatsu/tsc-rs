import json, sys, base64
B64='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
def vlq(s):
    out=[]; shift=0; val=0
    for ch in s:
        d=B64.index(ch); cont=d&32; d&=31; val|=d<<shift; shift+=5
        if not cont:
            neg=val&1; val>>=1; out.append(-val if neg else val); val=0; shift=0
    return out
def segments(m):
    rows=[]; sl=sc=nm=0
    for li,line in enumerate(m['mappings'].split(';')):
        gc=0
        for seg in filter(None, line.split(',')):
            f=vlq(seg); gc+=f[0]
            if len(f)>1: sf=f[1]; sl+=f[2]; sc+=f[3]
            rows.append((li,gc,sl,sc) if len(f)>1 else (li,gc))
            if len(f)>4: nm+=f[4]
    return rows
def load(p):
    return json.load(open(p))
a=segments(load(sys.argv[1])); b=segments(load(sys.argv[2]))
print('segments', len(a), len(b))
import difflib
for line in difflib.unified_diff([str(x) for x in a],[str(x) for x in b], sys.argv[1].split('/')[-1], sys.argv[2].split('/')[-1], lineterm='', n=1):
    print(line)
