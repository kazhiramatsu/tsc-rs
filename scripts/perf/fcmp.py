#!/usr/bin/env python3
"""Compare two samply profiles (symbol level, no inline expansion).
fcmp.py A.json.gz B.json.gz [--top N] [--by self|incl] [--grep RE]"""
import gzip, json, re, sys, argparse
from collections import defaultdict
ap = argparse.ArgumentParser(); ap.add_argument('a'); ap.add_argument('b'); ap.add_argument('--top', type=int, default=45); ap.add_argument('--by', default='self'); ap.add_argument('--grep')
args = ap.parse_args()
def demangle(s):
    s = re.sub(r'::h[0-9a-f]{16}$', '', s)
    s = s.replace('$LT$', '<').replace('$GT$', '>').replace('$u20$', ' ').replace('..', '::')
    return s
def load(path):
    p = json.load(gzip.open(path, 'rt'))
    syms = json.load(open(path.replace('.json.gz', '.json.syms.json')))
    st = syms['string_table']
    by = {}
    for e in syms['data']:
        table = e['symbol_table']
        m = {}
        for addr, idx in e['known_addresses']:
            m[addr] = st[table[idx]['symbol']] if idx is not None and idx < len(table) else hex(addr)
        by[e['debug_name']] = m
    self_ = defaultdict(int); incl = defaultdict(int); total = 0
    libs = [l['name'] for l in p['libs']]
    for t in p['threads']:
        ft, fn, rt, stt = t['frameTable'], t['funcTable'], t['resourceTable'], t['stackTable']
        def frame_name(fi):
            res = fn['resource'][ft['func'][fi]]
            a = ft['address'][fi]
            if res is None or res < 0: return '?'
            lib = rt['lib'][res]
            if lib is None or lib < 0: return '?'
            return demangle(by.get(libs[lib], {}).get(a, libs[lib] + ':' + hex(a)))
        names = {}
        stacks = {}
        def stack(si):
            r = stacks.get(si)
            if r is None:
                pre = stt['prefix'][si]
                r = (stack(pre) if pre is not None else ()) + (frame_name(stt['frame'][si]),)
                stacks[si] = r
            return r
        sys.setrecursionlimit(100000)
        w = t['samples'].get('weight') or [1] * len(t['samples']['stack'])
        for si, wt in zip(t['samples']['stack'], w):
            if si is None: continue
            s = stack(si)
            wt = wt or 1
            total += wt
            self_[s[-1]] += wt
            for n in set(s): incl[n] += wt
    return total, self_, incl
ta, sa, ia = load(args.a); tb, sb, ib = load(args.b)
print('total', ta, tb)
A, B = (sa, sb) if args.by == 'self' else (ia, ib)
names = set(A) | set(B)
if args.grep: names = {n for n in names if re.search(args.grep, n)}
print(f"{'A':>7} {'B':>7} {'B-A':>7}  name")
rows = sorted(names, key=lambda n: -(B.get(n, 0) - A.get(n, 0)))
for n in rows[:args.top]:
    print(f"{A.get(n,0):7d} {B.get(n,0):7d} {B.get(n,0)-A.get(n,0):7d}  {n[:100]}")
print('...')
for n in rows[-args.top:]:
    print(f"{A.get(n,0):7d} {B.get(n,0):7d} {B.get(n,0)-A.get(n,0):7d}  {n[:100]}")
