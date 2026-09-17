#!/usr/bin/env python3
"""Compare native dumps (TSC_RS_POST_T1_RESIDUALS_DUMP_DIR) with the frozen
upstream observations: per case, per write, byte equality; JavaScript text
diff and decoded source-map segment diff for differing writes.

usage: python3 analyze-dumps.py <fixture.json> <dump-dir> [--case <substr>]... [--full]
"""
import base64, difflib, hashlib, json, os, sys
B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
def vlq(s):
    out = []; shift = 0; val = 0
    for ch in s:
        d = B64.index(ch); cont = d & 32; d &= 31; val |= d << shift; shift += 5
        if not cont:
            neg = val & 1; val >>= 1; out.append(-val if neg else val); val = 0; shift = 0
    return out
def segments(m):
    rows = []; sl = sc = 0
    for li, line in enumerate(m['mappings'].split(';')):
        gc = 0
        for seg in filter(None, line.split(',')):
            f = vlq(seg); gc += f[0]
            if len(f) > 1: sl += f[2]; sc += f[3]
            rows.append((li, gc, sl, sc) if len(f) > 1 else (li, gc))
    return rows
fixture, dump = sys.argv[1], sys.argv[2]
needles = []; full = False
args = sys.argv[3:]
while args:
    a = args.pop(0)
    if a == '--case': needles.append(args.pop(0))
    elif a == '--full': full = True
data = json.load(open(fixture))
summary = {'exact': [], 'differs': [], 'missing': []}
for case in data['cases']:
    cid = case['case_id']
    if needles and not any(n in cid for n in needles): continue
    key = hashlib.sha256(cid.encode()).hexdigest()[:16]
    if not os.path.exists(os.path.join(dump, key + '.case_id')):
        summary['missing'].append(cid); continue
    exact = True; report = []
    for w in case['typescript_observation']['writes']:
        name = w['path'].rsplit('/', 1)[-1]
        expected = base64.b64decode(w['callback_utf8_base64'])
        native_path = None
        for f in os.listdir(dump):
            if f.startswith(key + '-1-') and f.endswith('-' + name): native_path = os.path.join(dump, f)
        if native_path is None:
            report.append(f'  {name}: NATIVE WRITE MISSING'); exact = False; continue
        native = open(native_path, 'rb').read()
        if native == expected: continue
        exact = False
        report.append(f'  {name}: differs (native {len(native)} B, upstream {len(expected)} B)')
        if name.endswith('.map'):
            a = segments(json.loads(native)); b = segments(json.loads(expected))
            for line in difflib.unified_diff([str(x) for x in a], [str(x) for x in b], 'native', 'upstream', lineterm='', n=1):
                report.append('    ' + line)
        else:
            for line in difflib.unified_diff(native.decode('utf-8', 'replace').splitlines(), expected.decode('utf-8', 'replace').splitlines(), 'native', 'upstream', lineterm='', n=1 if not full else 3):
                report.append('    ' + line)
    (summary['exact'] if exact else summary['differs']).append(cid)
    if not exact:
        print('=====', cid); print('\n'.join(report))
print(json.dumps({k: len(v) for k, v in summary.items()}))
for cid in summary['differs']: print('DIFFERS', cid)
for cid in summary['missing']: print('MISSING', cid)
