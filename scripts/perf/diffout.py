#!/usr/bin/env python3
"""Compare the output of two compiler binaries on the bench corpora.

diffout.py A_BIN B_BIN CASE[,CASE..]
Runs both one thread at a time (TSRS_WORKERS=1 TSRS_CHECKERS=1), compares the
exit code, stdout and stderr byte for byte, and reports the first differing line.
CASE = project/variant as in serial.py.
"""
import os, subprocess, sys
BENCH = os.path.expanduser(os.environ.get('TSRS_BENCH_ROOT', '~/dev/bench-corpora'))
ROOTS = {
    'hono': 'hono', 'zod': 'zod', 'effect': 'effect.noindex/packages/effect',
    'ts': 'TypeScript-6.0/src/compiler', 'next': 'next.js/packages/next',
    'vscode': 'vscode/src', 'playwright': 'playwright', 'vue': 'vue',
}
def run(binary, case):
    project, variant = case.split('/')
    cwd = os.path.join(BENCH, ROOTS[project])
    env = dict(os.environ, TSRS_WORKERS='1', TSRS_CHECKERS='1')
    p = subprocess.run([binary, '-p', f'tsconfig.bench-{variant}.json', '--pretty', 'false'],
                       cwd=cwd, capture_output=True, env=env)
    return p.returncode, p.stdout, p.stderr
def main():
    a, b, cases = sys.argv[1], sys.argv[2], sys.argv[3].split(',')
    same = True
    for case in cases:
        ra, oa, ea = run(a, case)
        rb, ob, eb = run(b, case)
        if ra == rb and oa == ob and ea == eb:
            print(f'{case:18} same  rc {ra}  {len(oa)} bytes  stderr {len(ea)} bytes', flush=True)
            continue
        if ra == rb and oa == ob:
            same = False
            print(f'{case:18} DIFFERENT stderr  {len(ea)} vs {len(eb)} bytes', flush=True)
            continue
        same = False
        print(f'{case:18} DIFFERENT  rc {ra} vs {rb}  {len(oa)} vs {len(ob)} bytes', flush=True)
        la, lb = oa.decode(errors='replace').splitlines(), ob.decode(errors='replace').splitlines()
        for i, (x, y) in enumerate(zip(la, lb)):
            if x != y:
                print(f'  line {i + 1}:\n  A: {x[:200]}\n  B: {y[:200]}')
                break
        else:
            print(f'  one output is a prefix of the other ({len(la)} vs {len(lb)} lines)')
    sys.exit(0 if same else 1)
main()
