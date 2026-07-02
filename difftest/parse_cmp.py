import sys
def load(p):
    d={}
    for line in open(p):
        line=line.rstrip("\n")
        if "\x01" not in line: continue
        path,codes=line.split("\x01",1)
        # set of codes (ignore offset) and full set (with offset)
        cs=set(); full=set()
        if codes and codes not in ("READERR","PANIC"):
            for c in codes.split(","):
                if ":" in c:
                    code,off=c.split(":",1); cs.add(code); full.add((code,off))
                else:
                    cs.add(c)
        d[path]=(cs,full,codes)
    return d
tsrs=load("/tmp/conf_out.txt")
tsc=load("/tmp/conf_tsc.txt")
fp=[]   # tsrs reports parse error, tsc clean
miss=[] # tsc reports parse error, tsrs clean
diff=[] # both have errors but differ
for path in tsrs:
    if path not in tsc: continue
    rs,rfull,rraw=tsrs[path]
    cs,cfull,craw=tsc[path]
    if rraw in ("PANIC","READERR") or craw=="READERR": continue
    if cs==set() and rs!=set(): fp.append(path)
    elif rs==set() and cs!=set(): miss.append(path)
    elif rs!=cs or rfull!=cfull:
        if rs or cs: diff.append(path)
print(f"FALSE-POSITIVE (tsc clean, tsrs errors): {len(fp)}")
print(f"MISSED (tsc errors, tsrs clean): {len(miss)}")
print(f"DIFFER (both error, mismatch): {len(diff)}")
# write lists
open("/tmp/fp.txt","w").write("\n".join(fp))
open("/tmp/miss.txt","w").write("\n".join(miss))
open("/tmp/diff.txt","w").write("\n".join(diff))
# show which CODES appear in false-positive files (the tsrs spurious codes)
from collections import Counter
c=Counter()
for path in fp:
    for code in tsrs[path][0]:
        c[code]+=1
print("\nFalse-positive tsrs codes:", c.most_common(15))
