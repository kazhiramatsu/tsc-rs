#!/usr/bin/env python3
"""Top immediate callers of symbols matching a regex in a samply profile.
callers.py PROFILE.json.gz REGEX [--depth N] [--top N]"""
import gzip, json, re, sys, argparse
from collections import defaultdict
ap = argparse.ArgumentParser(); ap.add_argument('p'); ap.add_argument('re'); ap.add_argument('--depth', type=int, default=1); ap.add_argument('--top', type=int, default=30)
args = ap.parse_args()
def dem(s):
    s = re.sub(r'::h[0-9a-f]{16}$', '', s)
    return s.replace('$LT$', '<').replace('$GT$', '>').replace('$u20$', ' ').replace('..', '::')
p = json.load(gzip.open(args.p, 'rt'))
syms = json.load(open(args.p.replace('.json.gz', '.json.syms.json')))
st = syms['string_table']; by = {}
for e in syms['data']:
    t = e['symbol_table']; m = {}
    for addr, idx in e['known_addresses']:
        m[addr] = st[t[idx]['symbol']] if idx is not None and idx < len(t) else hex(addr)
    by[e['debug_name']] = m
libs = [l['name'] for l in p['libs']]
pat = re.compile(args.re); tot = defaultdict(int); selft = 0; incl = 0
sys.setrecursionlimit(100000)
for t in p['threads']:
    ft, fn, rt, stt = t['frameTable'], t['funcTable'], t['resourceTable'], t['stackTable']
    def name(fi):
        res = fn['resource'][ft['func'][fi]]
        if res is None or res < 0: return '?'
        lib = rt['lib'][res]
        if lib is None or lib < 0: return '?'
        return dem(by.get(libs[lib], {}).get(ft['address'][fi], libs[lib] + ':' + hex(ft['address'][fi])))
    memo = {}
    def stack(si):
        r = memo.get(si)
        if r is None:
            pre = stt['prefix'][si]
            r = (stack(pre) if pre is not None else ()) + (name(stt['frame'][si]),)
            memo[si] = r
        return r
    w = t['samples'].get('weight') or [1] * len(t['samples']['stack'])
    for si, wt in zip(t['samples']['stack'], w):
        if si is None: continue
        s = stack(si); wt = wt or 1
        idx = [i for i, n in enumerate(s) if pat.search(n)]
        if not idx: continue
        incl += wt
        i = idx[0]
        if pat.search(s[-1]): selft += wt
        tot[' <- '.join(reversed(s[max(0, i - args.depth):i]))] += wt
print('incl', incl, 'self', selft)
for k, v in sorted(tot.items(), key=lambda kv: -kv[1])[:args.top]:
    print(f'{v:6d}  {k[:170]}')
