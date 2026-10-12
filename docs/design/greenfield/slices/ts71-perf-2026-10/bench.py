#!/usr/bin/env python3
"""Interleaved wall-clock comparison of tsc-rs and tsgo on one bench corpus.

bench.py PROJECT ROUNDS [VARIANT...]
  1. fidelity: one run of each binary per variant; stdout, exit code and the
     emitted tree (the variant's outDir) must be identical;
  2. timing: ROUNDS interleaved runs (tsc-rs, tsgo) per variant, each under
     /usr/bin/time -l (wall, user+sys CPU, peak RSS); medians are reported.
Results append to perf/results.jsonl.
"""
import json, os, re, shutil, statistics, subprocess, sys, time, filecmp
BENCH = os.path.expanduser('~/dev/bench-corpora')
RS = '/Users/hiramatsu/dev/tsc-rs-p34/target/release/tsc-rs'
TSGO = '/Users/hiramatsu/dev/tsc-rs/target/typescript7/bin/tsc-aa814927'
HERE = os.path.dirname(os.path.abspath(__file__))
PROJECTS = {
    'hono': ('hono', ['noemit', 'emit', 'decl', 'sourcemap', 'full']),
    'zod': ('zod', ['noemit', 'emit', 'decl', 'sourcemap', 'full']),
    'effect': ('effect.noindex/packages/effect', ['noemit', 'emit', 'decl', 'sourcemap', 'full']),
    'typescript': ('TypeScript-6.0/src/compiler', ['noemit', 'emit', 'decl', 'sourcemap', 'full']),
    'next': ('next.js/packages/next', ['noemit', 'emit', 'decl', 'sourcemap', 'full']),
    'vscode': ('vscode/src', ['stable']),
}

def config(variant):
    return f'tsconfig.bench-{variant}.json'

def out_dir(cwd, variant):
    path = os.path.join(cwd, config(variant))
    text = open(path).read()
    m = re.search(r'"outDir"\s*:\s*"([^"]+)"', text)
    return os.path.normpath(os.path.join(cwd, m.group(1))) if m else None

def timed(binary, cwd, variant, single=False):
    cmd = ['/usr/bin/time', '-l', binary, '-p', config(variant), '--pretty', 'false']
    env = dict(os.environ)
    if single:
        # The real-project reference: tsgo --singleThreaded against one
        # tsc-rs checker (multi-checker diagnostics depend on partitioning,
        # in tsgo too).
        if binary == TSGO:
            cmd.append('--singleThreaded')
        else:
            env['TSRS_CHECKERS'] = '1'
    t0 = time.perf_counter()
    p = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, env=env)
    wall = time.perf_counter() - t0
    err = p.stderr
    def field(name):
        m = re.search(r'(\d+(?:\.\d+)?)\s+' + name, err)
        return float(m.group(1)) if m else None
    user = re.search(r'([\d.]+) user', err); sys_ = re.search(r'([\d.]+) sys', err)
    cpu = (float(user.group(1)) if user else 0) + (float(sys_.group(1)) if sys_ else 0)
    rss = field('maximum resident set size')
    stderr_tool = err[:err.find('real')] if 'real' in err else ''
    return wall, cpu, rss, p.returncode, p.stdout, stderr_tool

def tree(path):
    files = {}
    if path and os.path.isdir(path):
        for root, _, names in os.walk(path):
            for n in names:
                full = os.path.join(root, n)
                files[os.path.relpath(full, path)] = open(full, 'rb').read()
    return files

def fidelity(cwd, variant):
    od = out_dir(cwd, variant)
    if od: shutil.rmtree(od, ignore_errors=True)
    _, _, _, go_rc, go_out, _ = timed(TSGO, cwd, variant)
    go_tree = tree(od)
    if od: shutil.rmtree(od, ignore_errors=True)
    _, _, _, rs_rc, rs_out, _ = timed(RS, cwd, variant)
    rs_tree = tree(od)
    diffs = []
    if go_rc != rs_rc: diffs.append(f'exit {go_rc} vs {rs_rc}')
    if go_out != rs_out: diffs.append(f'stdout differs ({len(go_out.splitlines())} vs {len(rs_out.splitlines())} lines)')
    names = set(go_tree) | set(rs_tree)
    differing = sorted(n for n in names if go_tree.get(n) != rs_tree.get(n))
    if differing: diffs.append(f'{len(differing)}/{len(names)} emitted files differ: {differing[:5]}')
    if diffs:
        # The real-project reference: tsgo --singleThreaded against one
        # tsc-rs checker (multi-checker results depend on partitioning).
        if od: shutil.rmtree(od, ignore_errors=True)
        _, _, _, go_rc1, go_out1, _ = timed(TSGO, cwd, variant, single=True)
        go_tree1 = tree(od)
        if od: shutil.rmtree(od, ignore_errors=True)
        _, _, _, rs_rc1, rs_out1, _ = timed(RS, cwd, variant, single=True)
        rs_tree1 = tree(od)
        names1 = set(go_tree1) | set(rs_tree1)
        differing1 = sorted(n for n in names1 if go_tree1.get(n) != rs_tree1.get(n))
        if go_out1 == rs_out1 and go_rc1 == rs_rc1 and not differing1:
            diffs = ['partition-dependent only (tsgo single-threaded = tsc-rs one checker: SAME)']
        else:
            single = []
            if go_rc1 != rs_rc1: single.append(f'exit {go_rc1} vs {rs_rc1}')
            if go_out1 != rs_out1: single.append('stdout')
            if differing1: single.append(f'{len(differing1)} files {differing1[:5]}')
            diffs.append('SINGLE-CHECKER DIFF: ' + ', '.join(single))
    return {'files': len(names), 'go_rc': go_rc, 'diffs': diffs,
            'go_out_head': go_out.splitlines()[:3], 'rs_out_head': rs_out.splitlines()[:3]}

def main():
    project, rounds = sys.argv[1], int(sys.argv[2])
    rel, variants = PROJECTS[project]
    if len(sys.argv) > 3: variants = sys.argv[3:]
    cwd = os.path.join(BENCH, rel)
    results = []
    for variant in variants:
        fid = fidelity(cwd, variant)
        print(f'[{project}/{variant}] fidelity: {"SAME" if not fid["diffs"] else "DIFF " + "; ".join(fid["diffs"])} ({fid["files"]} files, exit {fid["go_rc"]})', flush=True)
        if rounds == 0:
            continue
        samples = {'tsc-rs': [], 'tsgo': []}
        for _ in range(rounds):
            for label, binary in (('tsc-rs', RS), ('tsgo', TSGO)):
                wall, cpu, rss, rc, _, _ = timed(binary, cwd, variant)
                samples[label].append((wall, cpu, rss))
        def med(label, i):
            return statistics.median(s[i] for s in samples[label] if s[i] is not None)
        row = {'project': project, 'variant': variant, 'rounds': rounds, 'fidelity': fid['diffs'] or 'SAME',
               'rs_wall': med('tsc-rs', 0), 'go_wall': med('tsgo', 0),
               'rs_cpu': med('tsc-rs', 1), 'go_cpu': med('tsgo', 1),
               'rs_rss_mb': med('tsc-rs', 2) / 2**20, 'go_rss_mb': med('tsgo', 2) / 2**20,
               'samples': samples, 'time': time.strftime('%Y-%m-%d %H:%M:%S')}
        row['ratio'] = row['rs_wall'] / row['go_wall']
        print(f'[{project}/{variant}] wall tsc-rs {row["rs_wall"]:.3f}s tsgo {row["go_wall"]:.3f}s ratio {row["ratio"]:.2f} | cpu {row["rs_cpu"]:.2f}s/{row["go_cpu"]:.2f}s | rss {row["rs_rss_mb"]:.0f}/{row["go_rss_mb"]:.0f} MB', flush=True)
        with open(os.path.join(HERE, 'results.jsonl'), 'a') as f:
            f.write(json.dumps(row) + '\n')
        results.append(row)

if __name__ == '__main__':
    main()
