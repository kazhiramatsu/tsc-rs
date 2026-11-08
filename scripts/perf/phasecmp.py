#!/usr/bin/env python3
"""Phase and leaf-name comparison of two samply profiles (tsc-rs and tsrs).

phasecmp.py A.json.gz B.json.gz [--top N] [--roots] [--grep RE]
Each sample is charged to the outermost frame whose leaf name is a phase root
(parse / bind / check / emit / load), else to "other". Functions are compared by
their leaf name (the last path segment without generics, closures and hashes),
so the two ports' functions with the same tsgo name line up. --roots lists the
heaviest inclusive leaf names of both, to choose roots.
"""
import argparse, gzip, json, re, sys
from collections import defaultdict
ap = argparse.ArgumentParser()
ap.add_argument('a'); ap.add_argument('b')
ap.add_argument('--top', type=int, default=40)
ap.add_argument('--roots', action='store_true')
ap.add_argument('--grep'); ap.add_argument('--ratios', action='store_true')
args = ap.parse_args()
PHASES = [
    ('check', re.compile(r'^(check_source_file|check_source_file_worker|check_snapshot_serially|check_source_element|checkSourceFile)$')),
    ('bind', re.compile(r'^(bind_source_file|bindSourceFile|bind)$')),
    ('parse', re.compile(r'^(parse_source_file|parse_source_file_from_snapshot|parse_source_file_worker|ParseSourceFile)$')),
    ('emit', re.compile(r'^(emit|emit_files|emit_program)$')),
]
def demangle(s):
    s = re.sub(r'::h[0-9a-f]{16}$', '', s)
    s = s.replace('$LT$', '<').replace('$GT$', '>').replace('$u20$', ' ').replace('$C$', ',').replace('$RF$', '&').replace('..', '::')
    return s
def leaf(name):
    n = re.sub(r'<[^<>]*>', '', name)
    while '<' in n:
        m = re.sub(r'<[^<>]*>', '', n)
        if m == n: break
        n = m
    n = re.sub(r'::\{\{closure\}\}', '', n)
    n = re.sub(r'\{[^}]*\}', '', n)
    parts = [p for p in n.split('::') if p]
    return parts[-1] if parts else name
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
    self_ = defaultdict(int); incl = defaultdict(int); phase = defaultdict(int); total = 0
    phase_self = defaultdict(lambda: defaultdict(int))
    libs = [l['name'] for l in p['libs']]
    scale = float(p['meta'].get('interval') or 1.0)
    work = max(p['threads'], key=lambda t: len(t['samples']['stack']))
    for t in [work]:
        ft, fn, rt, stt = t['frameTable'], t['funcTable'], t['resourceTable'], t['stackTable']
        cache = {}
        def frame_name(fi):
            res = fn['resource'][ft['func'][fi]]
            a = ft['address'][fi]
            if res is None or res < 0: return '?'
            lib = rt['lib'][res]
            if lib is None or lib < 0: return '?'
            return leaf(demangle(by.get(libs[lib], {}).get(a, libs[lib] + ':' + hex(a))))
        stacks = {}
        def stack(si):
            r = stacks.get(si)
            if r is None:
                chain = []
                s = si
                while s is not None and s not in stacks:
                    chain.append(s); s = stt['prefix'][s]
                base = stacks[s] if s is not None else ()
                for c in reversed(chain):
                    base = base + (frame_name(stt['frame'][c]),)
                    stacks[c] = base
                r = stacks[si]
            return r
        w = t['samples'].get('weight') or [1] * len(t['samples']['stack'])
        for si, wt in zip(t['samples']['stack'], w):
            if si is None: continue
            s = stack(si); wt = (wt or 1) * scale; total += wt
            self_[s[-1]] += wt
            for n in set(s): incl[n] += wt
            ph = 'other'
            for f in s:
                hit = next((name for name, rx in PHASES if rx.match(f)), None)
                if hit: ph = hit; break
            phase[ph] += wt
            phase_self[ph][s[-1]] += wt
    return total, self_, incl, phase, phase_self
ta, sa, ia, pa, psa = load(args.a); tb, sb, ib, pb, psb = load(args.b)
print(f'work thread ms  A {ta:.0f}  B {tb:.0f}  ratio {ta / tb:.2f}')
if args.roots:
    for label, incl in (('A', ia), ('B', ib)):
        print(f'-- {label} heaviest inclusive leaf names')
        for n, v in sorted(incl.items(), key=lambda kv: -kv[1])[:args.top]:
            print(f'{v:7.0f}  {n[:90]}')
    sys.exit()
print(f"{'phase':8} {'A':>7} {'B':>7} {'A/B':>6} {'A-B':>7}")
for ph in ('load', 'parse', 'bind', 'check', 'emit', 'other'):
    a, b = pa.get(ph, 0), pb.get(ph, 0)
    print(f"{ph:8} {a:7.0f} {b:7.0f} {a / b if b else float('nan'):6.2f} {a - b:7.0f}")
names = set(sa) | set(sb)
if args.grep: names = {n for n in names if re.search(args.grep, n)}
print(f"\nself by leaf name, largest A-B first\n{'A':>7} {'B':>7} {'A-B':>7}  name")
for n in sorted(names, key=lambda n: -(sa.get(n, 0) - sb.get(n, 0)))[:args.top]:
    print(f"{sa.get(n, 0):7.0f} {sb.get(n, 0):7.0f} {sa.get(n, 0) - sb.get(n, 0):7.0f}  {n[:90]}")
if '--ratios' in sys.argv:
    common = [n for n in set(ia) & set(ib) if ia[n] >= 150 and ib[n] >= 150]
    print(f"\ninclusive ms of functions present in both (>=150 ms each), A/B ratio")
    rows = sorted(common, key=lambda n: -ib[n])
    import statistics
    rs = []
    for n in rows:
        r = ia[n] / ib[n]; rs.append(r)
        print(f"{ia[n]:7.0f} {ib[n]:7.0f} {r:5.2f}  {n[:80]}")
    print(f"median ratio {statistics.median(rs):.2f}  (n={len(rs)})")
