#!/usr/bin/env python3
"""children.py PROF.json.gz PARENT_LEAF [--depth 1] [--top 25]: inclusive ms of the frames directly below the outermost
frame whose leaf name is PARENT_LEAF, on the busiest thread (leaf names as in phasecmp.py)."""
import argparse, gzip, json, re, sys
from collections import defaultdict
sys.argv_saved = list(sys.argv)
ap = argparse.ArgumentParser(); ap.add_argument('p'); ap.add_argument('parent'); ap.add_argument('--top', type=int, default=25)
args = ap.parse_args()
src = open('phasecmp.py').read()
ns = {"re": re}
exec(src.split('def load(path):')[0].split("args = ap.parse_args()")[1], ns)
demangle, leaf = ns['demangle'], ns['leaf']
p = json.load(gzip.open(args.p, 'rt'))
syms = json.load(open(args.p.replace('.json.gz', '.json.syms.json')))
st = syms['string_table']; by = {}
for e in syms['data']:
    table = e['symbol_table']; m = {}
    for addr, idx in e['known_addresses']:
        m[addr] = st[table[idx]['symbol']] if idx is not None and idx < len(table) else hex(addr)
    by[e['debug_name']] = m
libs = [l['name'] for l in p['libs']]
scale = float(p['meta'].get('interval') or 1.0)
t = max(p['threads'], key=lambda t: len(t['samples']['stack']))
ft, fn, rt, stt = t['frameTable'], t['funcTable'], t['resourceTable'], t['stackTable']
def frame_name(fi):
    res = fn['resource'][ft['func'][fi]]; a = ft['address'][fi]
    if res is None or res < 0: return '?'
    lib = rt['lib'][res]
    if lib is None or lib < 0: return '?'
    return leaf(demangle(by.get(libs[lib], {}).get(a, libs[lib] + ':' + hex(a))))
stacks = {}
def stack(si):
    chain = []; s = si
    while s is not None and s not in stacks:
        chain.append(s); s = stt['prefix'][s]
    base = stacks[s] if s is not None else ()
    for c in reversed(chain):
        base = base + (frame_name(stt['frame'][c]),); stacks[c] = base
    return stacks[si]
kids = defaultdict(float); total = 0.0
w = t['samples'].get('weight') or [1] * len(t['samples']['stack'])
for si, wt in zip(t['samples']['stack'], w):
    if si is None: continue
    s = stack(si)
    if args.parent not in s: continue
    i = s.index(args.parent); wt = (wt or 1) * scale; total += wt
    kids[s[i + 1] if i + 1 < len(s) else '(self)'] += wt
print(f'{args.parent}: {total:.0f} ms')
for n, v in sorted(kids.items(), key=lambda kv: -kv[1])[:args.top]:
    print(f'{v:7.0f}  {n[:90]}')
