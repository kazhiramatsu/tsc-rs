#!/usr/bin/env python3
"""Shared hosted PGO recipe. Never run build/train in the PGO editing lane.

The PR experiment and the release use the same build, serial training,
profile validation and merge implementation. Only the corpus selection differs.
"""
import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import urllib.request

RECIPE_ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(RECIPE_ROOT/'scripts'))
import pgo_build

PINS = dict(hono='8dcd52b92beada28962e818bdca8a137a6beb656',
            typescript='050880ce59e30b356b686bd3144efe24f875ebc8', zod='2bf7b06')
PROFILE = '7.1.0-dev-aa814927'
UPSTREAM = 'aa8149273b23401f0a79a5f0384c42de51888693'


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_json(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')


def inventory(root):
    return {p.relative_to(root).as_posix():
            {'symlink': os.readlink(p)} if p.is_symlink() else {'sha256': sha256(p)}
            for p in sorted(root.rglob('*'))
            if '.git' not in p.relative_to(root).parts and p != root/'manifest.json'
            and (p.is_symlink() or p.is_file())}


def verify_corpus(corpora):
    manifest = json.loads((corpora/'manifest.json').read_text())
    if inventory(corpora) != manifest['files']:
        raise RuntimeError('frozen corpus bytes changed')
    return manifest


def freeze(corpora, recipe):
    root = corpora
    for name, pin in [('hono', PINS['hono']),
                      ('typescript', PINS['typescript'])]:
        actual = subprocess.check_output(['git', '-C', str(root/name),
                                          'rev-parse', 'HEAD'], text=True).strip()
        if actual != pin:
            raise SystemExit(f'{name}: checkout SHA mismatch')
    lock = json.loads((root/'typescript/package-lock.json').read_text())
    dependencies = {}
    # Both projects use this same closure. No npm/pnpm install, ranges,
    # lifecycle scripts, generated upstream compiler, or ambient typings.
    for name, version in [('@types/node', '25.3.3'), ('undici-types', '7.18.2')]:
        item = lock['packages']['node_modules/' + name]
        assert item['version'] == version
        assert item['resolved'].startswith('https://registry.npmjs.org/')
        assert item['integrity'].startswith('sha512-')
        with urllib.request.urlopen(item['resolved'], timeout=90) as response:
            data = response.read()
        actual = base64.b64encode(hashlib.sha512(data).digest()).decode()
        assert 'sha512-' + actual == item['integrity'], name
        destination = root/'types/node_modules'/name
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory() as temporary:
            with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
                archive.extractall(temporary, filter='data')
            entries = list(Path(temporary).iterdir())
            assert len(entries) == 1 and entries[0].is_dir()
            shutil.move(str(entries[0]), destination)
        dependencies[name] = {k: item[k] for k in ['version', 'resolved', 'integrity']}
    for project in ['hono', 'typescript']:
        (root/project/'node_modules').symlink_to('../types/node_modules')
    common = dict(composite=False, incremental=False, noEmit=True,
                  declaration=False, declarationMap=False, sourceMap=False)
    configs = {
        'hono/tsconfig.ci-pgo.json': {
            'extends': './tsconfig.build.json', 'compilerOptions': common},
        'typescript/src/compiler/tsconfig.ci-pgo.json': {
            'extends': './tsconfig.json',
            'compilerOptions': dict(common, emitDeclarationOnly=False,
                                    isolatedDeclarations=False, rootDir='.'),
            'exclude': ['dist-bench.noindex']}}
    for name, config in configs.items():
        (root/name).write_text(json.dumps(config, indent=2) + '\n')
    if recipe == 'release':
        zod = root/'zod'
        # The benchmark record pins this abbreviated commit. Resolve it only from
        # the full checkout; ambiguous or absent objects fail, never follow HEAD.
        subprocess.run(['git', '-C', str(zod), 'checkout', '--detach', PINS['zod']], check=True)
        actual = subprocess.check_output(['git', '-C', str(zod), 'rev-parse', 'HEAD'], text=True).strip()
        if not actual.startswith(PINS['zod']):
            raise RuntimeError('zod: checkout SHA mismatch')
        source = zod/'packages/zod/src'
        if not source.is_dir():
            raise RuntimeError('zod: missing production source tree')
        configs['zod/tsconfig.ci-pgo.json'] = dict(
            compilerOptions=dict(common, target='ESNext', module='NodeNext',
                                 moduleResolution='NodeNext', strict=True,
                                 skipLibCheck=True, types=[], lib=['ESNext', 'DOM']),
            include=['packages/zod/src/**/*.ts'],
            exclude=['**/tests/**', '**/__tests__/**', '**/bench/**', '**/benchmarks/**',
                     '**/*.test.ts', '**/*.spec.ts'])
        (zod/'tsconfig.ci-pgo.json').write_text(json.dumps(configs['zod/tsconfig.ci-pgo.json'], indent=2)+'\n')
        batches = root/'conformance'
        batches.mkdir()
        for listing in sorted((RECIPE_ROOT/'.github/pgo').glob('*.cases')):
            shutil.copy2(listing, batches/listing.name)
        if len(list(batches.glob('*.cases'))) != 2:
            raise RuntimeError('expected exactly two fixed conformance batches')
    else:
        actual = None
    manifest = dict(recipe=recipe, hono=PINS['hono'], typescript=PINS['typescript'],
                    zod=actual, dependencies=dependencies,
                    conformance_profile=PROFILE, conformance_commit=UPSTREAM,
                    configs={name: sha256(root/name) for name in configs},
                    batches={p.name: sha256(p) for p in sorted((root/'conformance').glob('*.cases'))})
    # Include every source/declaration/config byte, independent of tar timestamps
    # and checkout metadata. Symlink targets are recorded as well.
    manifest['files'] = inventory(root)
    write_json(root/'manifest.json', manifest)
    print(json.dumps({k: v for k, v in manifest.items() if k != 'files'}, indent=2))


def build(source, out, kind, jobs):
    out.mkdir(parents=True, exist_ok=True)
    (out/'logs').mkdir(exist_ok=True)
    toolchain = tomllib.loads((source/'rust-toolchain.toml').read_text())['toolchain']['channel']
    rustc = subprocess.check_output(['rustc', '-vV'], cwd=source, text=True)
    if not rustc.startswith(f'rustc {toolchain} '):
        raise RuntimeError('rustc must match rust-toolchain.toml')
    vendor = source/'vendor/typescript-native'/PROFILE/'manifest.json'
    if json.loads(vendor.read_text())['commit'] != UPSTREAM:
        raise RuntimeError('vendored conformance profile changed; review the recipe')
    options = dict(root=source, locked=True)
    if kind == 'instrumented':
        # Fresh training each time, including after an object-cache hit.
        shutil.rmtree(out/'profiles', ignore_errors=True)
        pgo_build.instrumented_build(str(out), jobs,
            packages=('tsc-rs-cli', 'tsc-rs-conformance'),
            bins=('tsc-rs', 'conformance-ts71', 'suites-ts71'), **options)
        return
    flags = '' if kind == 'normal' else (
        f'-Cprofile-use={out}/merged.profdata -Cllvm-args=-pgo-warn-missing-function')
    if kind == 'pgo' and not (out/'merged.profdata').is_file():
        raise RuntimeError('missing merged training profile')
    tree = 'normal' if kind == 'normal' else 'optimized'
    binary = pgo_build.cargo_build(out/tree, flags, jobs, **options)
    shutil.copy2(binary, out/'tsc-rs')


def training_run(command, cwd, out, name, timeout, project=False):
    env = pgo_build.profile_environment(str(out/'profiles'), f'train-{name}-%p-%m.profraw')
    env.update(TSRS_WORKERS='1', TSRS_CHECKERS='1', CARGO_NET_OFFLINE='true')
    with (out/'logs'/f'{name}.log').open('w') as log:
        result = subprocess.run(list(map(str, command)), cwd=cwd, env=env,
                                stdout=log, stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode not in ((0, 1, 2) if project else (0,)):
        raise RuntimeError(f'{name}: abnormal exit {result.returncode}; see training log')
    if project and re.search(r'TS(?:18003|5058|5083|6053)\b',
                             (out/'logs'/f'{name}.log').read_text()):
        raise RuntimeError(f'{name}: missing training inputs/configuration')
    raw = list((out/'profiles').glob(f'train-{name}-*.profraw'))
    if not raw or any(p.stat().st_size == 0 for p in raw):
        raise RuntimeError(f'{name}: missing/empty training profile (CLI must flush before exit)')
    print(f'{name}: exit {result.returncode}; {len(raw)} profiles', flush=True)


def train(source, out, corpora, recipe):
    manifest = verify_corpus(corpora)
    if manifest['recipe'] != recipe:
        raise RuntimeError('training recipe/corpus mismatch')
    binary = out/'instrumented/release'
    # Listing is not a training pass and must not inherit a caller's filename.
    env = dict(os.environ)
    env.pop('LLVM_PROFILE_FILE', None)
    cases = sorted(subprocess.check_output([str(binary/'conformance-ts71'),
                    '--profile', PROFILE, '--list'], cwd=source, env=env, text=True).splitlines())
    if len(cases) < 12000:
        raise RuntimeError('unexpectedly small conformance corpus')
    if recipe == 'release':
        batches = sorted((corpora/'conformance').glob('*.cases'))
        if len(batches) != 2:
            raise RuntimeError('expected two conformance batches')
        for listing in batches:
            selected = listing.read_text().splitlines()
            if len(selected) != 128 or len(set(selected)) != 128 or not set(selected) <= set(cases):
                raise RuntimeError(f'{listing}: fixed batch is absent from this vendor profile')
    else:
        batches = []
        for index, start in enumerate(range(0, len(cases), 128)):
            listing = out/'logs'/f'conformance-{index:04d}.cases'
            listing.write_text(''.join(case+'\n' for case in cases[start:start+128]))
            batches.append(listing)
    passes = []
    for listing in batches:
        saved = out/'logs'/listing.name
        if listing != saved:
            shutil.copy2(listing, saved)
        training_run([binary/'conformance-ts71', '--profile', PROFILE, '--checkers', '1',
                      '--worker', saved], source, out, listing.stem, 300)
        passes.append(dict(name=listing.stem, cases_sha256=sha256(saved)))
    if recipe == 'experiment':
        training_run([binary/'suites-ts71', '--profile', PROFILE, '--no-report'],
                     source, out, 'suites', 900)
    projects = ['hono', 'zod'] if recipe == 'release' else ['hono', 'typescript']
    for name in projects:
        directory = corpora/name
        if name == 'typescript':
            directory /= 'src/compiler'
        config = directory/'tsconfig.ci-pgo.json'
        if not config.is_file():
            raise RuntimeError(f'{name}: missing training tsconfig')
        training_run([binary/'tsc-rs', '-p', config, '--noEmit', '--pretty', 'false'],
                     directory, out, name, 300, project=True)
    verify_corpus(corpora)
    write_json(out/'logs/training.json', dict(recipe=recipe, workers=1, checkers=1,
               batches=passes, projects=projects, corpus_sha256=sha256(corpora/'manifest.json')))


def merge(source, out):
    tool = pgo_build.llvm_profdata()
    raw = sorted((out/'profiles').glob('train-*.profraw'))
    if not raw or any(p.stat().st_size == 0 for p in raw):
        raise RuntimeError('no nonempty training profiles')
    subprocess.run([tool, 'merge', '-o', str(out/'merged.profdata'), *map(str, raw)],
                   cwd=source, check=True)
    with (out/'logs/profile-summary.txt').open('w') as log:
        subprocess.run([tool, 'show', str(out/'merged.profdata')], stdout=log, check=True)
    print(f'{len(raw)} training profiles; merged sha256={sha256(out/"merged.profdata")}')
    shutil.rmtree(out/'instrumented')


def package(source, out, corpora, kind, dist):
    corpus = verify_corpus(corpora)
    rustc = subprocess.check_output(['rustc', '-vV'], cwd=source, text=True)
    host = next(line.split(': ', 1)[1] for line in rustc.splitlines() if line.startswith('host: '))
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=source, text=True).strip()
    release = tomllib.loads((source/'Cargo.toml').read_text())['profile']['release']
    # Record effective defaults as well as the explicit manifest settings.
    settings = dict(lto=release.get('lto', False), codegen_units=release.get('codegen-units', 16),
                    opt_level=release.get('opt-level', 3), debug=release.get('debug', False),
                    panic=release.get('panic', 'unwind'), incremental=False)
    overrides = [key for key in os.environ if key.startswith('CARGO_PROFILE_RELEASE_')]
    if overrides:
        raise RuntimeError(f'unrecorded release profile overrides: {overrides}')
    name = f'tsc-rs-{host}-{kind}'
    dist.mkdir(parents=True, exist_ok=True)
    stage = dist/name
    stage.mkdir()
    shutil.copy2(out/'tsc-rs', stage/'tsc-rs')
    merged = out/'merged.profdata'
    flags = '' if kind == 'normal' else f'-Cprofile-use={merged} -Cllvm-args=-pgo-warn-missing-function'
    metadata = dict(schema=1, kind=kind, commit=commit, target=host, rustc=rustc,
                    toolchain=tomllib.loads((source/'rust-toolchain.toml').read_text())['toolchain'],
                    release_profile=settings, rustflags=flags,
                    recipe_sha256=sha256(Path(__file__)), helper_sha256=sha256(RECIPE_ROOT/'scripts/pgo_build.py'),
                    cargo_lock_sha256=sha256(source/'Cargo.lock'), binary_sha256=sha256(stage/'tsc-rs'),
                    vendor_manifest_sha256=sha256(source/'vendor/typescript-native'/PROFILE/'manifest.json'),
                    corpus_sha256=sha256(corpora/'manifest.json'), training_corpus=corpus,
                    training=json.loads((out/'logs/training.json').read_text()) if kind == 'pgo' else None,
                    profdata_sha256=sha256(merged) if kind == 'pgo' else None,
                    profdata_version=subprocess.check_output([pgo_build.llvm_profdata(), '--version'], text=True),
                    runner=dict(system=platform.platform(), image=os.environ.get('ImageVersion'),
                                arch=os.environ.get('RUNNER_ARCH')),
                    run=dict(repository=os.environ.get('GITHUB_REPOSITORY'), id=os.environ.get('GITHUB_RUN_ID'),
                             attempt=os.environ.get('GITHUB_RUN_ATTEMPT'), workflow_sha=os.environ.get('WORKFLOW_SHA')))
    write_json(stage/'BUILD.json', metadata)
    shutil.copy2(stage/'BUILD.json', dist/f'{name}.BUILD.json')
    (stage/'SHA256SUMS').write_text(''.join(f'{sha256(stage/f)}  {f}\n' for f in ['tsc-rs', 'BUILD.json']))
    with tarfile.open(dist/f'{name}.tar.gz', 'w:gz') as archive:
        archive.add(stage, arcname=name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('step', choices=['pins', 'freeze', 'build', 'train', 'merge', 'package'])
    parser.add_argument('--source', type=Path, default=Path.cwd())
    parser.add_argument('--out', type=Path)
    parser.add_argument('--corpora', type=Path, default=Path('corpora'))
    parser.add_argument('--recipe', choices=['experiment', 'release'], default='experiment')
    parser.add_argument('--kind', choices=['instrumented', 'normal', 'pgo'], default='pgo')
    parser.add_argument('--jobs', type=int, default=2)
    parser.add_argument('--dist', type=Path, default=Path('dist'))
    args = parser.parse_args()
    source = args.source.resolve()
    out = args.out.resolve() if args.out else source/'target/pgo'
    corpora = args.corpora.resolve()
    if args.step == 'pins':
        values = dict(PINS, toolchain=tomllib.loads((source/'rust-toolchain.toml').read_text())['toolchain']['channel'])
        with open(os.environ['GITHUB_OUTPUT'], 'a') as output:
            output.write(''.join(f'{k}={v}\n' for k, v in values.items()))
    elif args.step == 'freeze':
        freeze(corpora, args.recipe)
    elif args.step == 'build':
        build(source, out, args.kind, args.jobs)
    elif args.step == 'train':
        train(source, out, corpora, args.recipe)
    elif args.step == 'merge':
        merge(source, out)
    elif args.step == 'package':
        package(source, out, corpora, args.kind, args.dist.resolve())


if __name__ == '__main__':
    main()
