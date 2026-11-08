#!/usr/bin/env python3
"""One-core comparison of compiler binaries on the bench corpora.

serial.py OUT.jsonl BIN_LABEL=BIN[,..] CASE[,..] [--rounds N] [--warmup N]
A binary whose label starts with "tsgo" or "tsrs" runs with --singleThreaded;
any other runs with TSRS_WORKERS=1 TSRS_CHECKERS=1. CASE = project/variant.
One process at a time, each under /usr/bin/time -l. Per case, every binary
first runs --warmup times unmeasured (default 1), then the binaries run
interleaved for --rounds measured rounds (default 3). Each measured run is
printed and appended to OUT.jsonl; the end prints the median per (case,
label) and the change against the first label. Instructions and memory are
the numbers to judge by (wall time is noisy on a shared machine).

Why the warm-up (2026-11-04): the first run of a project pays its cold start
(page cache, kernel work counted in the process's retired instructions), up
to +2.5% of instructions on the small projects and +0.86 G on VS Code; with
the base listed first, a single unwarmed round charged that to the base.
"""
import json, os, re, statistics, subprocess, sys, time
BENCH = os.path.expanduser(os.environ.get('TSRS_BENCH_ROOT', '~/dev/bench-corpora'))
ROOTS = {
    'hono': 'hono', 'zod': 'zod', 'effect': 'effect.noindex/packages/effect',
    'ts': 'TypeScript-6.0/src/compiler', 'next': 'next.js/packages/next',
    'vscode': 'vscode/src', 'playwright': 'playwright', 'vue': 'vue',
}
def run(label, binary, case):
    project, variant = case.split('/')
    cwd = os.path.join(BENCH, ROOTS[project])
    cmd = ['/usr/bin/time', '-l', binary, '-p', f'tsconfig.bench-{variant}.json', '--pretty', 'false']
    env = dict(os.environ)
    if label.startswith(('tsgo', 'tsrs')):
        cmd.append('--singleThreaded')
    else:
        env['TSRS_WORKERS'] = '1'; env['TSRS_CHECKERS'] = '1'
    t0 = time.perf_counter()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env)
    wall = time.perf_counter() - t0
    err = p.stderr
    def f(name):
        m = re.search(r'(\d+)\s+' + name, err)
        return int(m.group(1)) if m else None
    user = float(re.search(r'([\d.]+) user', err).group(1)); sys_ = float(re.search(r'([\d.]+) sys', err).group(1))
    return dict(label=label, case=case, wall=wall, cpu=user + sys_, instr=f('instructions retired'),
                rss=f('maximum resident set size'), foot=f('peak memory footprint'),
                reclaims=f('page reclaims'), rc=p.returncode,
                out=len(p.stdout), t=time.strftime('%H:%M:%S'))
def option(name, default):
    return int(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default
def main():
    out, bins, cases = sys.argv[1], sys.argv[2].split(','), sys.argv[3].split(',')
    rounds, warmup = option('--rounds', 3), option('--warmup', 1)
    pairs = [b.split('=', 1) for b in bins]
    runs = {}
    for case in cases:
        for _ in range(warmup):
            for label, binary in pairs:
                run(label, binary, case)
        for _ in range(rounds):
            for label, binary in pairs:
                r = run(label, binary, case)
                runs.setdefault((case, label), []).append(r)
                with open(out, 'a') as fh: fh.write(json.dumps(r) + '\n')
                print(f"{case:18} {label:14} rc {r['rc']} wall {r['wall']:7.2f}s cpu {r['cpu']:7.2f}s instr {r['instr']/1e9:8.2f}G rss {r['rss']/2**20:7.0f}M foot {r['foot']/2**20:7.0f}M out {r['out']}", flush=True)
    print(f"\nmedians of {rounds} measured round(s) after {warmup} warm-up run(s) per binary; change against {pairs[0][0]}")
    for case in cases:
        base = None
        for label, _ in pairs:
            rs = runs[(case, label)]
            med = {k: statistics.median(r[k] for r in rs) for k in ('instr', 'cpu', 'foot', 'rss')}
            if base is None:
                base, change = med, ''
            else:
                change = f"instr {100 * (med['instr'] - base['instr']) / base['instr']:+6.2f}%  foot {(med['foot'] - base['foot']) / 2**20:+6.0f}M"
            print(f"{case:18} {label:14} instr {med['instr']/1e9:8.2f}G cpu {med['cpu']:7.2f}s foot {med['foot']/2**20:7.0f}M rss {med['rss']/2**20:7.0f}M  {change}", flush=True)
main()
