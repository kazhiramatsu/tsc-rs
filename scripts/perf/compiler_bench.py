#!/usr/bin/env python3
"""Prepare isolated inputs and compare fresh compiler processes on macOS/Linux.

No builds, downloads, historical ratchet updates, or CI registration. Actual
compiler runs happen only with the explicit `run` subcommand.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import statistics
import subprocess
import sys
import threading
import time


MODES = {
    'check': ['--noEmit'],
    'emit': [],
    'full-emit': ['--declaration', '--declarationMap', '--sourceMap'],
}
NATIVE_MODES = {
    'default': ([], {}),
    'gomaxprocs-1': ([], {'GOMAXPROCS': '1'}),
    'checkers-1': (['--checkers', '1'], {}),
    'single-threaded': (['--singleThreaded'], {}),
}


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + '\n')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def files(root):
    result = {}
    for path in sorted(root.rglob('*')):
        if path.is_symlink():
            raise ValueError(f'symlink requires separate input review: {path}')
        if path.is_file():
            result[path.relative_to(root).as_posix()] = digest(path)
    return result


def prepare(root):
    root.mkdir(parents=True, exist_ok=False)
    scenarios = []

    def case(name, sources, control=False):
        folder = root / name
        folder.mkdir()
        for file, source in sources.items():
            (folder / file).write_text(source)
        write_json(folder / 'tsconfig.json', {
            'compilerOptions': {'target': 'ES2022', 'module': 'ESNext',
                                'moduleResolution': 'Bundler', 'strict': True,
                                'lib': ['es2022'], 'types': [], 'skipLibCheck': True},
            'files': sorted(sources),
        })
        scenarios.append({'name': name, 'control': control,
                          'source_files': sorted(sources), 'inputs': files(folder)})

    case('control', {'index.ts': 'const value: number = "wrong";\nexport { value };\n'}, True)
    case('startup', {'index.ts': 'export const value: number = 42;\n'})
    for count in (64, 512):
        sources = {'common.ts': 'export interface Box<T> { value: T; }\n'}
        imports, calls = [], []
        for i in range(count):
            sources[f'module{i}.ts'] = (
                'import type { Box } from "./common";\n'
                f'export interface Item{i} {{ id: number; name: string; }}\n'
                f'export function select{i}(item: Box<Item{i}>): string {{\n'
                '  return item.value.name;\n}\n')
            imports.append(f'import {{ select{i} }} from "./module{i}";')
            calls.append(f'  select{i}({{ value: {{ id: {i}, name: "item" }} }}),')
        sources['index.ts'] = '\n'.join(imports + ['export const names: string[] = ['] + calls + ['];', ''])
        case(f'modules-{count}', sources)
    source = ['type ReadonlyFields<T> = { readonly [K in keyof T]: T[K] };',
              'type WithId<T> = ReadonlyFields<T> & { id: number };']
    for i in range(256):
        source.append(f'export const item{i}: WithId<{{ name: string; tag{i}: number }}> = '
                      f'{{ id: {i}, name: "item", tag{i}: {i} }};')
    case('types', {'index.ts': '\n'.join(source) + '\n'})
    write_json(root / 'suite.json', {
        'schema': 1, 'kind': 'generated-synthetic',
        'library_policy': 'Each compiler uses its own bundled ES2022 libraries; versions differ.',
        'scenarios': scenarios,
    })
    return root / 'suite.json'


def removed_environment_keys():
    return sorted(key for key in os.environ
                  if key in {'GOMAXPROCS', 'GOGC', 'GOMEMLIMIT', 'GODEBUG', 'GOCOVERDIR',
                             'GOFLAGS', 'NODE_OPTIONS', 'RAYON_NUM_THREADS', 'RUST_LOG',
                             'GLIBC_TUNABLES', 'LD_PRELOAD', 'DYLD_INSERT_LIBRARIES'}
                  or key.startswith(('TSRS_', 'TSC_RS_', 'MALLOC_', 'Malloc')))


def environment(overrides):
    removed = set(removed_environment_keys())
    env = {key: value for key, value in os.environ.items() if key not in removed}
    env.update(overrides)
    env['NO_COLOR'] = '1'
    return env


def priority():
    result = {'nice': os.getpriority(os.PRIO_PROCESS, 0), 'darwin_background': None}
    if result['nice'] != 0:
        raise ValueError('Run measurements at normal priority, outside demoted development shells.')
    if sys.platform == 'darwin':
        if not hasattr(os, 'PRIO_DARWIN_PROCESS'):
            raise ValueError('Python >= 3.12 is required to check Darwin background policy.')
        # Darwin getpriority returns 0/1 here, NOT the PRIO_DARWIN_BG (4096)
        # setpriority flag. Verified with taskpolicy -b on the measurement host.
        result['darwin_background'] = os.getpriority(os.PRIO_DARWIN_PROCESS, 0)
        if result['darwin_background'] != 0:
            raise ValueError('Darwin background policy is active; start outside taskpolicy -b.')
    return result


def measure(argv, cwd, env, log_base, timeout):
    """wait4 measures this child, not a cumulative maximum over earlier runs."""
    timed_out = threading.Event()
    with log_base.with_suffix('.stdout').open('wb') as stdout, log_base.with_suffix('.stderr').open('wb') as stderr:
        start = time.perf_counter()
        process = subprocess.Popen(argv, cwd=cwd, env=env, stdout=stdout,
                                   stderr=stderr, start_new_session=True)

        def stop():
            timed_out.set()
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass

        timer = threading.Timer(timeout, stop)
        timer.daemon = True
        timer.start()
        try:
            _, status, usage = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
        except BaseException:
            stop()
            _, status, _ = os.wait4(process.pid, 0)
            process.returncode = os.waitstatus_to_exitcode(status)
            raise
        finally:
            timer.cancel()
        elapsed = time.perf_counter() - start
    return {'argv': argv, 'exit': process.returncode, 'timeout': timed_out.is_set(),
            'wall_seconds': elapsed, 'user_seconds': usage.ru_utime,
            'system_seconds': usage.ru_stime,
            'peak_rss_bytes': int(usage.ru_maxrss * (1 if sys.platform == 'darwin' else 1024)),
            'stdout': log_base.with_suffix('.stdout').name,
            'stderr': log_base.with_suffix('.stderr').name}


def distribution(values):
    center = statistics.median(values)
    return {'median': center, 'min': min(values), 'max': max(values),
            'median_absolute_deviation': statistics.median(abs(x - center) for x in values)}


def hardware():
    if sys.platform == 'darwin':
        return {key: subprocess.check_output(['sysctl', '-n', key], text=True).strip()
                for key in ('hw.model', 'machdep.cpu.brand_string', 'hw.memsize')}
    cpu = Path('/proc/cpuinfo').read_text()
    memory = Path('/proc/meminfo').read_text()
    return {'cpu_model': next((x.split(':', 1)[1].strip() for x in cpu.splitlines()
                               if x.startswith('model name')), platform.machine()),
            'memory': next(x for x in memory.splitlines() if x.startswith('MemTotal:')),
            'cpu_affinity': sorted(os.sched_getaffinity(0))}


def source_ref(path):
    return {'path': str(path),
            'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=path, text=True).strip(),
            'working_tree_status': subprocess.check_output(
                ['git', 'status', '--porcelain', '--untracked-files=all'], cwd=path, text=True)}


def run(args):
    if sys.platform not in ('darwin', 'linux'):
        raise ValueError('This runner supports macOS/Linux wait4/RSS conventions only.')
    initial_priority = priority()
    suite_path = args.suite.resolve()
    suite = json.loads(suite_path.read_text())
    if suite.get('schema') != 1 or suite.get('kind') != 'generated-synthetic':
        raise ValueError('Use the generated suite. External projects need their own input/configuration review.')
    scenarios = suite['scenarios']
    names = [x['name'] for x in scenarios]
    if len(set(names)) != len(names) or any(not re.fullmatch(r'[a-z0-9-]+', x) for x in names):
        raise ValueError('Invalid or duplicate scenario name')
    selected = args.cases.split(',') if args.cases else [x['name'] for x in scenarios if not x['control']]
    if not selected or len(set(selected)) != len(selected) or set(selected) - set(names) or 'control' in selected:
        raise ValueError('Select existing non-control scenarios')
    modes = args.modes.split(',')
    if not modes or len(set(modes)) != len(modes) or set(modes) - set(MODES):
        raise ValueError('Unknown compilation mode')
    binaries = {'rust': args.rust.resolve(), 'tsgo': args.tsgo.resolve()}
    if any(not p.is_file() or not os.access(p, os.X_OK) for p in binaries.values()):
        raise ValueError('Provide built, executable compilers; the runner never builds them.')
    if args.output.is_relative_to(suite_path.parent):
        raise ValueError('Keep result/output directories outside the prepared input suite.')
    args.output.mkdir(parents=True, exist_ok=False)
    logs = args.output / 'logs'
    logs.mkdir()
    output = args.output / 'compiler-output'
    output.mkdir()
    native_args, native_env = NATIVE_MODES[args.tsgo_mode]
    report = {'status': 'running', 'performance_qualification': False,
              'scope': 'Fresh processes, warm filesystem; synthetic sources, compiler-specific bundled libraries.',
              'runner_sha256': digest(Path(__file__)),
              'suite': suite, 'suite_sha256': digest(suite_path), 'runs': [], 'comparisons': [],
              'host': {'platform': platform.platform(), 'machine': platform.machine(),
                       'hardware': hardware(), 'python': platform.python_version(),
                       'cpu_count': os.cpu_count(), 'load_average_start': os.getloadavg(),
                       'priority': initial_priority},
              'repeats': args.repeats, 'warmups': args.warmups, 'tsgo_mode': args.tsgo_mode,
              'environment': {'removed_inherited_keys': removed_environment_keys(),
                              'rust_overrides': {'NO_COLOR': '1'},
                              'tsgo_overrides': {'NO_COLOR': '1', **native_env}},
              'binaries': {}}

    def save():
        write_json(args.output / 'results.json', report)

    def check_inputs():
        if digest(suite_path) != report['suite_sha256']:
            raise ValueError('Suite manifest changed during the run')
        for case in scenarios:
            if files(suite_path.parent / case['name']) != case['inputs']:
                raise ValueError(f'Input mutation or stale suite: {case["name"]}')
        for name, info in report['binaries'].items():
            if digest(binaries[name]) != info['sha256']:
                raise ValueError(f'Compiler changed during the run: {name}')

    def invoke(case, mode, compiler, phase, index):
        priority_before = priority()
        for child in output.iterdir():
            if child.is_dir() and not child.is_symlink():
                shutil.rmtree(child)
            else:
                child.unlink()
        folder = suite_path.parent / case['name']
        argv = [str(binaries[compiler]), '-p', str(folder / 'tsconfig.json'),
                '--pretty', 'false', '--outDir', str(output), *MODES[mode]]
        if compiler == 'tsgo':
            argv += native_args
        log_base = logs / f'{len(report["runs"]):04d}-{case["name"]}-{mode}-{compiler}-{phase}-{index}'
        row = measure(argv, folder, environment(native_env if compiler == 'tsgo' else {}), log_base, args.timeout)
        row.update(case=case['name'], mode=mode, compiler=compiler, phase=phase, index=index,
                   priority_before=priority_before,
                   outputs=files(output))
        report['runs'].append(row)
        save()
        priority()
        stdout = (logs / row['stdout']).read_bytes()
        stderr = (logs / row['stderr']).read_bytes()
        if row['timeout']:
            raise ValueError(f'Timeout: {log_base.name}')
        if case['control']:
            if row['exit'] <= 0 or re.findall(rb'error TS\d+\b', stdout) != [b'error TS2322'] or stderr or row['outputs']:
                raise ValueError(f'Type-error control failed: {log_base.name}')
        else:
            if row['exit'] != 0 or stdout or stderr:
                raise ValueError(f'Compilation failed or produced diagnostics: {log_base.name}')
            suffixes = [] if mode == 'check' else ['.js']
            if mode == 'full-emit':
                suffixes += ['.js.map', '.d.ts', '.d.ts.map']
            expected = {str(Path(p).with_suffix('')) + suffix
                        for p in case['source_files'] for suffix in suffixes}
            if set(row['outputs']) != expected:
                raise ValueError(f'Missing or unexpected output files: {log_base.name}')
        return row

    try:
        for name, binary in binaries.items():
            version = subprocess.run([str(binary), '--version'], capture_output=True, timeout=args.timeout,
                                     env=environment({}), check=True)
            report['binaries'][name] = {'path': str(binary), 'sha256': digest(binary),
                                       'version': version.stdout.decode(errors='replace').strip(),
                                       'source': source_ref(getattr(args, name + '_source'))}
        check_inputs()
        controls = [x for x in scenarios if x['control']]
        if len(controls) != 1:
            raise ValueError('Exactly one type-error control is required')
        for compiler in binaries:
            invoke(controls[0], 'check', compiler, 'control', 0)
        check_inputs()
        for case in scenarios:
            if case['name'] not in selected:
                continue
            for mode in modes:
                reference = None
                for index in range(args.warmups + args.repeats):
                    phase = 'warmup' if index < args.warmups else 'measured'
                    order = ['rust', 'tsgo'] if index % 2 == 0 else ['tsgo', 'rust']
                    for compiler in order:
                        row = invoke(case, mode, compiler, phase, index)
                        if reference is None:
                            reference = row['outputs']
                        elif row['outputs'] != reference:
                            raise ValueError(f'Output bytes differ: {case["name"]}/{mode}/{compiler}')
                    check_inputs()
                print(f'verified {case["name"]}/{mode}', flush=True)
        check_inputs()
        for case in selected:
            for mode in modes:
                summary = {'case': case, 'mode': mode, 'samples_per_compiler': args.repeats}
                for compiler in binaries:
                    rows = [r for r in report['runs'] if r['case'] == case and r['mode'] == mode
                            and r['compiler'] == compiler and r['phase'] == 'measured']
                    summary[compiler] = {
                        'wall_seconds': distribution([r['wall_seconds'] for r in rows]),
                        'cpu_seconds': distribution([r['user_seconds'] + r['system_seconds'] for r in rows]),
                        'peak_rss_bytes': distribution([r['peak_rss_bytes'] for r in rows]),
                    }
                summary['rust_over_tsgo_wall_ratio'] = summary['rust']['wall_seconds']['median'] / summary['tsgo']['wall_seconds']['median']
                for metric in ('cpu_seconds', 'peak_rss_bytes'):
                    denominator = summary['tsgo'][metric]['median']
                    summary['rust_over_tsgo_' + metric + '_ratio'] = summary['rust'][metric]['median'] / denominator if denominator else None
                report['comparisons'].append(summary)
        report['status'] = 'complete'
    except (Exception, KeyboardInterrupt) as error:
        report.update(status='invalid', error=f'{type(error).__name__}: {error}', comparisons=[])
    report['host']['load_average_end'] = os.getloadavg()
    save()
    print(args.output / 'results.json')
    if report['status'] != 'complete':
        print(report['error'], file=sys.stderr)
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    prep = sub.add_parser('prepare', help='write new synthetic inputs without running compilers')
    prep.add_argument('directory', type=Path)
    bench = sub.add_parser('run', help='explicitly execute a prepared comparison')
    for option in ('suite', 'rust', 'tsgo', 'rust-source', 'tsgo-source', 'output'):
        bench.add_argument('--' + option, type=Path, required=True)
    bench.add_argument('--cases', help='comma-separated subset; default: all four scenarios')
    bench.add_argument('--modes', default='check,emit,full-emit')
    bench.add_argument('--tsgo-mode', choices=NATIVE_MODES, default='default')
    bench.add_argument('--repeats', type=int, default=8)
    bench.add_argument('--warmups', type=int, default=1)
    bench.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args()
    if args.command == 'prepare':
        print(prepare(args.directory.resolve()))
        return 0
    if args.repeats < 2 or args.repeats % 2 or args.warmups < 1 or not math.isfinite(args.timeout) or args.timeout <= 0:
        parser.error('Use an even repeat count >= 2, at least one warmup, and a positive timeout.')
    args.output = args.output.resolve()
    return run(args)


if __name__ == '__main__':
    sys.exit(main())
