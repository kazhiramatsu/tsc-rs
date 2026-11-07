#!/usr/bin/env python3
"""Shared same-runner, serial A/B measurements; GNU time and Cachegrind Ir."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import time


def measure(root, out, project, release=False, counts=True):
    out.mkdir(parents=True, exist_ok=True)
    sides = ['normal', 'pgo'] if release else ['base', 'head']
    directory = root/'corpora'/project
    if project == 'typescript':
        directory /= 'src/compiler'
    base_env = dict(os.environ)
    for key in ['LLVM_PROFILE_FILE', 'TSRS_WORKERS', 'TSRS_CHECKERS', 'TSRS_MEMORY_REPORT']:
        base_env.pop(key, None)
    env_1t = dict(base_env, TSRS_WORKERS='1', TSRS_CHECKERS='1')
    records, outputs, metadata = [], {}, {}
    corpus = json.loads((root/'corpora/manifest.json').read_text())
    for side in sides:
        path = root/'binaries'/side
        metadata[side] = json.loads((path/('BUILD.json' if release else 'metadata.json')).read_text())
        if hashlib.sha256((path/'tsc-rs').read_bytes()).hexdigest() != metadata[side]['binary_sha256']:
            raise RuntimeError(f'{side}: binary hash mismatch')
        if metadata[side]['corpus_sha256'] != hashlib.sha256((root/'corpora/manifest.json').read_bytes()).hexdigest():
            raise RuntimeError(f'{side}: corpus hash mismatch')
    a, b = (metadata[side] for side in sides)
    keys = ['commit', 'rustc', 'target', 'release_profile', 'recipe_sha256', 'helper_sha256',
            'cargo_lock_sha256', 'vendor_manifest_sha256'] if release else [
            'kind', 'rustc', 'recipe', 'image', 'workflow', 'profdata_version',
            'corpus_sha256', 'vendor_manifest_sha256', 'rustflags']
    for key in keys:
        if a[key] != b[key]:
            raise RuntimeError(f'comparison contract differs: {key}')
    if not release and not a['image']:
        raise RuntimeError('missing runner image identity')
    if release and (a['kind'] != 'normal' or b['kind'] != 'pgo' or not b['profdata_sha256']):
        raise RuntimeError('expected normal then PGO binary with training provenance')
    def run(side, mode, iteration):
        binary = root/'binaries'/side/'tsc-rs'
        prefix = out/f'{side}-{mode}-{iteration}'
        command = [str(binary), '-p', 'tsconfig.ci-pgo.json', '--noEmit', '--pretty', 'false']
        env = base_env if mode == 'default' else env_1t
        if mode == 'count':
            command = ['valgrind', '--tool=cachegrind', '--cache-sim=no',
                       '--branch-sim=no', '--trace-children=no',
                       f'--cachegrind-out-file={prefix}.cg', f'--log-file={prefix}.valgrind',
                       *command]
        else:
            command = ['/usr/bin/time', '-v', '-o', f'{prefix}.time', *command]
        started = time.perf_counter()
        with Path(f'{prefix}.stdout').open('wb') as stdout, Path(f'{prefix}.stderr').open('wb') as stderr:
            result = subprocess.run(command, cwd=directory, env=env, stdout=stdout,
                                    stderr=stderr, timeout=1800 if mode == 'count' else 180)
        elapsed = time.perf_counter() - started
        if result.returncode not in (0, 1, 2):
            raise RuntimeError(f'{side}/{mode}: abnormal exit {result.returncode}')
        identity = (result.returncode, Path(f'{prefix}.stdout').read_bytes(),
                    Path(f'{prefix}.stderr').read_bytes())
        # Includes equality across native serial/default and Cachegrind,
        # and across base/head. Do not publish a speedup of changed output.
        if outputs and identity != next(iter(outputs.values())):
            raise RuntimeError(f'{side}/{mode}: diagnostic/exit identity changed')
        outputs[side, mode, iteration] = identity
        if re.search(rb'TS(?:18003|5058|5083|6053)\b', identity[1] + identity[2]):
            raise RuntimeError('Missing input/configuration; invalid measurement')
        row = dict(side=side, mode=mode, iteration=iteration, exit=result.returncode,
                   wall_seconds=elapsed, warmup=mode != 'count' and iteration == 0)
        if mode == 'count':
            lines = Path(f'{prefix}.cg').read_text().splitlines()
            events = next(line.split()[1:] for line in lines if line.startswith('events:'))
            summary = next(line.split()[1:] for line in lines if line.startswith('summary:'))
            row['ir'] = int(summary[events.index('Ir')])
            if row['ir'] <= 0:
                raise RuntimeError('empty instruction count')
            log = Path(f'{prefix}.valgrind').read_text()
            human = re.search(r'I\s+refs:\s*([0-9,]+)', log)
            assert human and int(human[1].replace(',', '')) == row['ir']
            print(f'{side}/{project}: I refs: {row["ir"]:,}', flush=True)
        else:
            log = Path(f'{prefix}.time').read_text()
            row['rss_kib'] = int(re.search(r'Maximum resident set size \(kbytes\):\s*(\d+)', log)[1])
        records.append(row)
        (out/'samples.json').write_text(json.dumps(records, indent=2) + '\n')
    # Warm both binaries outside the reported sample; then interleave.
    for mode in ['serial', 'default']:
        for iteration in range(4):
            for side in sides:
                run(side, mode, iteration)
    if counts:
        for iteration in range(2):
            for side in sides:
                run(side, 'count', iteration)
    summary = dict(project=project, metadata=metadata, corpus=corpus, sides={}, valid=True,
                   runner_image=os.environ.get('ImageVersion'),
                   valgrind=subprocess.check_output(['valgrind', '--version'], text=True).strip())
    for side in sides:
        data = {}
        for mode in ['serial', 'default']:
            selected = [r for r in records if r['side'] == side and r['mode'] == mode and r['iteration'] > 0]
            data[mode+'_wall_ms'] = statistics.median(r['wall_seconds'] for r in selected) * 1000
            data[mode+'_rss_kib'] = max(r['rss_kib'] for r in selected)
        counts = [r['ir'] for r in records if r['side'] == side and r['mode'] == 'count']
        data['ir_samples'] = counts
        data['ir'] = counts[0] if counts and len(set(counts)) == 1 else None
        if counts and len(set(counts)) != 1:
            summary['valid'] = False
        summary['sides'][side] = data
    (out/'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(summary, indent=2), flush=True)
    if not summary['valid']:
        raise SystemExit('Repeated I refs differ: retained raw samples; no sub-1% conclusion')

    return summary


def render_release(root, projects):
    lines = ['## Normal / PGO (same hosted runner)', '',
             '| Project | Binary | Cachegrind Ir (1 thread) | Serial wall median (ms) | Peak RSS (MiB) |',
             '| --- | --- | ---: | ---: | ---: |']
    for project in projects:
        data = json.loads((root/project/'summary.json').read_text())
        for side in ['normal', 'pgo']:
            row = data['sides'][side]
            lines.append(f"| {project} | {side} | {row['ir']:,} | {row['serial_wall_ms']:.2f} | {row['serial_rss_kib']/1024:.2f} |")
        a, b = (data['sides'][side]['ir'] for side in ['normal', 'pgo'])
        lines.append(f'| {project} | PGO / normal change | {(b/a-1)*100:+.4f}% | | |')
    lines += ['', 'One warmup then three interleaved native rounds (median wall, maximum RSS); '
              'two identical Cachegrind Ir repeats per binary; matching diagnostics and exit status required.',
              'Linux hosted counts are specific to this runner and corpus. They are not comparable to local macOS hardware counts. '
              'PGO is reported separately and never counted as an architectural gain.']
    text = '\n'.join(lines)+'\n'
    print(text)
    with open(os.environ['GITHUB_STEP_SUMMARY'], 'a') as summary:
        summary.write(text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', choices=['hono', 'zod', 'typescript'])
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--no-counts', action='store_true')
    parser.add_argument('--root', type=Path, default=Path.cwd())
    parser.add_argument('--out', type=Path, default=Path('results'))
    parser.add_argument('--summary', action='store_true')
    args = parser.parse_args()
    if args.summary:
        render_release(args.out, ['hono', 'zod'])
    else:
        measure(args.root.resolve(), args.out.resolve(), args.project,
                args.release, not args.no_counts)


if __name__ == '__main__':
    main()
