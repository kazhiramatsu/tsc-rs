import json, os, subprocess, sys, tempfile, shutil
from pathlib import Path

ROOT = Path(os.environ.get("TSRS_ROOT", Path(__file__).resolve().parents[1]))
B = os.environ.get("TSRS_BIN_RELEASE", str(ROOT / "target" / "release" / "tsrs"))
LIB = os.environ.get("TSRS_LIB", str(ROOT / "lib" / "lib.tsrs.d.ts"))
TSC_BATCH = os.environ.get("TSRS_TSC_BATCH", str(ROOT / "conf" / "tsc_batch.js"))
# lib-artifact codes to ignore (missing globals in curated lib)
LIB_CODES={2318,2304,2583,2584,2792}
cases=json.load(open(sys.argv[1]))
# tsc batch
tsc_res=tempfile.NamedTemporaryFile(delete=False)
tsc_res.close()
try:
    subprocess.run(['node', TSC_BATCH, sys.argv[1], tsc_res.name], check=True)
    tscr=json.load(open(tsc_res.name))
finally:
    try:
        os.unlink(tsc_res.name)
    except OSError:
        pass
# tsrs per-case
work=tempfile.mkdtemp()
shutil.copy(LIB, os.path.join(work,'lib.tsrs.d.ts'))
def norm(diags):
    # diags: list of [code,start,cat]; ignore lib codes; drop suggestions? keep but tag
    return sorted((c,s) for c,s,cat in diags if c not in LIB_CODES)
fp_cases=[]; fn_cases=[]; match=0
for c in cases:
    open(os.path.join(work,'main.ts'),'w').write(c['src'])
    env=dict(os.environ); env['TSRS_VIRTUAL_CWD']=work
    p=subprocess.run([B,'--strict','--diag-json','main.ts'],cwd=work,capture_output=True,text=True,env=env)
    try:
        tj=json.loads(p.stdout)
        td=[[d['code'],d.get('start'),d.get('category',1)] for d in tj['diagnostics'] if d.get('file')=='main.ts']
    except Exception as e:
        td=[['PARSE_ERR',0,1]]
    tsc_n=norm(tscr.get(c['name'],[]))
    tsrs_n=norm(td)
    fp=[x for x in tsrs_n if x not in tsc_n]
    fn=[x for x in tsc_n if x not in tsrs_n]
    if not fp and not fn: match+=1
    if fp: fp_cases.append((c['name'],fp,tsc_n,tsrs_n,c['src']))
    if fn: fn_cases.append((c['name'],fn,tsc_n,tsrs_n,c['src']))
shutil.rmtree(work)
print(f"TOTAL {len(cases)} | MATCH {match} | FP-cases {len(fp_cases)} | FN-cases {len(fn_cases)}")
print("\n=== FALSE POSITIVES (tsrs errors tsc doesn't) ===")
for n,fp,tsc,tsrs,src in fp_cases[:40]:
    print(f"[{n}] FP={fp}\n   src: {src}\n   tsc={tsc} tsrs={tsrs}")
if len(sys.argv)>2 and sys.argv[2]=='fn':
    print("\n=== FALSE NEGATIVES (tsrs misses) ===")
    for n,fn,tsc,tsrs,src in fn_cases[:40]:
        print(f"[{n}] FN={fn}\n   src: {src}\n   tsc={tsc} tsrs={tsrs}")
