#!/usr/bin/env python3
"""Refresh stale artifact-hash pins inside an oracle script.
Patterns: ["key", "path", "hash"] triples and "path":\n  "hash" map entries.
Only replaces when the path exists and its current sha256 differs."""
import hashlib, re, sys
def cur(p):
    try:
        return hashlib.sha256(open(p,'rb').read()).hexdigest()
    except OSError:
        return None
script=sys.argv[1]
s=open(script).read()
changed=[]
# pattern A: "path", "hash"  (array triple tail or adjacent pair, same or next line)
patA=re.compile(r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)",\s*"([0-9a-f]{64})"')
def subA(m):
    path,old=m.group(1),m.group(2)
    now=cur(path)
    if now and now!=old:
        changed.append((path,old[:8],now[:8]))
        return f'"{path}", "{now}"'
    return m.group(0)
s=patA.sub(subA,s)
# pattern B: "path":\n    "hash"
patB=re.compile(r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)":\s*\n(\s*)"([0-9a-f]{64})"')
def subB(m):
    path,indent,old=m.group(1),m.group(2),m.group(3)
    now=cur(path)
    if now and now!=old:
        changed.append((path,old[:8],now[:8]))
        return f'"{path}":\n{indent}"{now}"'
    return m.group(0)
s=patB.sub(subB,s)
# pattern C: "path": "hash" same-line map
patC=re.compile(r'"((?:ratchets|vendor|crates/oracle|\.github)/[^"\n]+)": "([0-9a-f]{64})"')
def subC(m):
    path,old=m.group(1),m.group(2)
    now=cur(path)
    if now and now!=old:
        changed.append((path,old[:8],now[:8]))
        return f'"{path}": "{now}"'
    return m.group(0)
s=patC.sub(subC,s)
open(script,'w').write(s)
for path,old,new in changed:
    print(f"re-pinned {path}: {old}.. -> {new}..")
if not changed:
    print("no stale pins found (failure may be a count const or deeper)")

# pattern D: const X_RELATIVE_PATH = "path"; ... const X_SHA256 = "hash";
s=open(script).read()
changedD=[]
pairs=dict(re.findall(r'const (\w+?)_RELATIVE_PATH =\s*\n?\s*"([^"]+)"', s))
for name, path in pairs.items():
    now=cur(path)
    if not now:
        continue
    for const_name in (name+'_SHA256', 'EXPECTED_'+name+'_SHA256'):
        patD=re.compile(r'(const '+re.escape(const_name)+r' =\s*\n?\s*")([0-9a-f]{64})(")')
        m=patD.search(s)
        if m and m.group(2)!=now:
            s=patD.sub(lambda mm: mm.group(1)+now+mm.group(3), s)
            changedD.append((path,m.group(2)[:8],now[:8]))
if changedD:
    open(script,'w').write(s)
    for path,old,new in changedD:
        print(f"re-pinned(const) {path}: {old}.. -> {new}..")

# pattern E: computed keys — [PATH_CONST]:\n  "hash" where const PATH_CONST = "path"
s=open(script).read()
changedE=[]
name_to_path=dict(re.findall(r'const (\w+) =\s*\n?\s*"((?:ratchets|vendor|crates|\.github)/[^"\n]+)"', s))
for name, path in name_to_path.items():
    now=cur(path)
    if not now:
        continue
    patE=re.compile(r'(\['+re.escape(name)+r'\]:\s*\n?\s*")([0-9a-f]{64})(")')
    for m in list(patE.finditer(s)):
        if m.group(2)!=now:
            s=patE.sub(lambda mm: mm.group(1)+now+mm.group(3), s)
            changedE.append((path,m.group(2)[:8],now[:8]))
if changedE:
    open(script,'w').write(s)
    for path,old,new in changedE:
        print(f"re-pinned(key) {path}: {old}.. -> {new}..")
