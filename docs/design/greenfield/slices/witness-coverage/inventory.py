#!/usr/bin/env python3
"""Static PR-gate entry inventory; Cargo metadata only, no build/test/oracle run.

Direct command templates are projected from witness.invocation, PRINTER_TARGETS
and replay's literal owner command. Rust module and fixture references are source
incidence, deliberately NOT proof that every test or fixture row executes.
"""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[5]
HERE = Path(__file__).resolve().parent
READ = set()


def read(path):
    path = path.resolve()
    READ.add(path)
    return path.read_text()


def relative(path):
    return str(path.resolve().relative_to(ROOT))


def module_paths(path):
    source = read(path)
    return [(name, (path.parent / spelling).resolve()) for spelling, name in re.findall(
        r'^\s*#\[path\s*=\s*"([^"\n]+)"\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;',
        source, re.M)]


def closure(path):
    found, pending = set(), [path.resolve()]
    while pending:
        path = pending.pop()
        if path in found:
            continue
        found.add(path)
        for _, child in module_paths(path):
            if not child.is_file():
                raise ValueError(f"missing module: {relative(path)} -> {child}")
            pending.append(child)
    return found


def load_replay():
    sys.path.insert(0, str(ROOT / '.github/ci'))
    import replay
    READ.update((ROOT / name).resolve() for name in (
        '.github/ci/replay.py', '.github/ci/test_replay.py', 'scripts/witness.py',
        '.github/workflows/ci.yml', '.github/workflows/witness.yml'))
    expected_runs = {
        'ci.yml': {'python3 .github/ci/replay.py plan', 'python3 .github/ci/replay.py acceptance "$ACCEPTANCE_GROUP"', 'python3 .github/ci/replay.py gate acceptance'},
        'witness.yml': {'python3 .github/ci/replay.py plan', 'python3 .github/ci/replay.py witnesses', 'python3 .github/ci/replay.py gate witnesses'},
    }
    for name, expected in expected_runs.items():
        runs = re.findall(r'^\s*run:\s*(.+)$', read(ROOT / '.github/workflows' / name), re.M)
        if set(runs) != expected or len(runs) != len(expected):
            raise ValueError(f'review changed workflow command roots: {name}: {runs}')
    return replay


def command_rows(replay):
    rows = []
    for group, suites in replay.WITNESS_GROUPS.items():
        for suite in suites:
            if suite == 'printer':
                for target in replay.PRINTER_TARGETS:
                    rows.append({'group': group, 'suite': suite, 'package': 'tsc-rs-emitter',
                                 'target': target, 'filter': None,
                                 'evidence': '.github/ci/replay.py:printer_witnesses/PRINTER_TARGETS'})
                continue
            argv, _ = replay.witness.invocation(suite, [], {})
            owner = argv[argv.index('--manifest-path') + 1].split('/')[1]
            if '--lib' in argv:
                rows.append({'group': group, 'suite': suite, 'package': f'tsc-rs-{owner}',
                             'target': f'tsc_{owner}', 'kind': 'lib', 'filter': None,
                             'evidence': 'scripts/witness.py:invocation'})
                argv = [arg for arg in argv if arg != '--lib']
            if '--exact' in argv:
                at = argv.index('--test')
                assert argv.count('--test') == 1 and argv[at + 2] != '--', argv
                # Extra libtest exact-name filters follow `--`; retain each
                # command membership instead of silently reporting only one.
                names = [argv[at + 2], *(arg for arg in argv[argv.index('--') + 1:]
                                       if not arg.startswith('--'))]
                assert len(names) == len(set(names)), argv
                targets = [(argv[at + 1], name) for name in names]
            else:
                options = argv[argv.index('--manifest-path') + 2:argv.index('--')]
                assert options and len(options) % 2 == 0 and options[::2] == ['--test'] * (len(options) // 2), argv
                targets = [(target, None) for target in options[1::2]]
            for target, test_filter in targets:
                rows.append({'group': group, 'suite': suite, 'package': f'tsc-rs-{owner}',
                             'target': target, 'filter': test_filter,
                             'evidence': 'scripts/witness.py:invocation'})
    module = ast.parse(read(ROOT / '.github/ci/replay.py'))
    printer = next(node for node in module.body if isinstance(node, ast.FunctionDef)
                   and node.name == 'printer_witnesses')
    literal_commands = []
    for node in ast.walk(printer):
        if not isinstance(node, ast.Call) or not node.args or not isinstance(node.args[0], ast.List):
            continue
        try:
            argv = ast.literal_eval(node.args[0])
        except (ValueError, TypeError):
            continue
        if argv[:2] == ['cargo', 'test']:
            literal_commands.append(argv)
    assert len(literal_commands) == 1, 'review new/removed literal printer owner commands'
    argv = literal_commands[0]
    at = argv.index('--test')
    rows.append({'group': 'printer', 'suite': 'printer', 'package': 'tsc-rs-emitter',
                 'target': argv[at + 1], 'filter': argv[at + 2],
                 'evidence': '.github/ci/replay.py:printer_witnesses/gate'})
    return rows


def acceptance_references():
    """Conservative file-level reference graph, not a Rust function reachability proof."""
    dispatch = ROOT / 'crates/xtask/src/acceptance_slices.rs'
    initial = set(re.findall(r'=> crate::(\w+)::', read(dispatch)))
    pending = [ROOT / f'crates/xtask/src/{name}.rs' for name in initial]
    drivers, references = set(), []
    while pending:
        driver = pending.pop().resolve()
        if driver in drivers or not driver.is_file():
            continue
        drivers.add(driver)
        source = read(driver)
        for name in re.findall(r'crate::(\w+)::', source):
            candidate = ROOT / f'crates/xtask/src/{name}.rs'
            if candidate.is_file():
                pending.append(candidate)
        for name, path in module_paths(driver):
            rel = relative(path)
            if rel.startswith('crates/xtask/tests/'):
                continue  # cfg(test) harness modules, not the dev-profile acceptance binary.
            if '/tests/' in rel:
                references.append({'driver': relative(driver), 'module': name, 'source': rel,
                                   'qualified_calls_in_driver': sorted(set(re.findall(
                                       rf'\b{re.escape(name)}::(\w+)\s*\(', source)))})
            else:
                pending.append(path)
    return sorted(relative(p) for p in drivers), sorted(references, key=lambda row: (row['source'], row['driver']))


def fixtures(paths, package_root):
    refs = set()
    unresolved = set()
    for path in sorted(paths):
        source = read(path)
        for spelling in re.findall(r'"([^"\n]*(?:fixtures/)[^"\n]+\.(?:json(?:\.zst)?|js|ts))"', source):
            if any(c in spelling for c in ('{', '}', '*')):
                unresolved.add(spelling)
                continue
            choices = [path.parent / spelling, package_root / spelling, ROOT / spelling]
            existing = [p.resolve() for p in choices if p.is_file()]
            if existing:
                refs.update(relative(p) for p in existing)
            else:
                unresolved.add(spelling)
    return sorted(refs), sorted(unresolved)


def inventory(source_commit):
    READ.clear()
    replay = load_replay()
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--offline', '--no-deps', '--format-version', '1'], cwd=ROOT, text=True))
    commands = command_rows(replay)
    drivers, references = acceptance_references()
    imported = {}
    for reference in references:
        for file in closure(ROOT / reference['source']):
            imported.setdefault(relative(file), []).append(reference)
    targets, harnesses = [], []
    for package in sorted(metadata['packages'], key=lambda row: row['name']):
        manifest = Path(package['manifest_path'])
        read(manifest)
        for target in sorted(package['targets'], key=lambda row: row['name']):
            source = Path(target['src_path'])
            if 'test' not in target['kind']:
                if target['test']:
                    harnesses.append({'package': package['name'], 'target': target['name'],
                                      'kind': target['kind'], 'source': relative(source),
                                      'direct_pr_test_command': any(row['package'] == package['name'] and row['target'] == target['name'] and row.get('kind') == 'lib' for row in commands)})
                continue
            sources = closure(source)
            selected = [row for row in commands if row['package'] == package['name'] and row['target'] == target['name']]
            mode = ('unfiltered-target-command' if any(row['filter'] is None for row in selected)
                    else 'filtered-target-command' if selected else 'no-direct-target-command')
            shared = {relative(path): imported[relative(path)] for path in sorted(sources)
                      if relative(path) in imported}
            literals, unresolved = fixtures(sources, manifest.parent)
            targets.append({'package': package['name'], 'target': target['name'],
                            'source': relative(source), 'direct_mode': mode, 'commands': selected,
                            'plan_for_target_source_change': replay.selection([relative(source)]),
                            'explicit_path_module_sources': sorted(relative(p) for p in sources),
                            'acceptance_shared_source_references': shared,
                            'literal_fixture_files': literals,
                            'unresolved_or_dynamic_fixture_spellings': unresolved})
    keys = {(row['package'], row['target']) for row in targets}
    keys.update((row['package'], row['target']) for row in harnesses)
    assert all((row['package'], row['target']) in keys for row in commands)
    for path in ('Cargo.toml', 'Cargo.lock', 'crates/xtask/src/acceptance_plan.rs'):
        read(ROOT / path)
    count = lambda mode: sum(row['direct_mode'] == mode for row in targets)
    return {
        'version': 1, 'source_commit': source_commit,
        'generator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'scope': 'configured PR-gate entries in ci.yml and witness.yml; no tests executed',
        'limits': [
            'An unfiltered target command does not prove ignored/cfg-disabled tests or every data row executes.',
            'A filtered target command does not run the entire binary, even if the current binary has one test.',
            'Acceptance shared-source references are conservative file-level incidence, not all-test execution proof.',
            'Only literal #[path] Rust module edges are followed; macro/generated/default-path modules need separate review.',
            'Literal fixture paths are source references, not dynamic fixture discovery or row membership proof.',
            'No direct target command does not mean its production behavior or shared acceptance helper is untested.',
            'lib/bin entries report configured cargo test harness commands in these two PR workflows, not behavior coverage.',
        ],
        'summary': {'standalone_targets': len(targets), 'lib_bin_harness_targets': len(harnesses),
                    'lib_bin_harnesses_with_direct_command': sum(row['direct_pr_test_command'] for row in harnesses),
                    'unfiltered_target_commands': count('unfiltered-target-command'),
                    'filtered_target_commands': count('filtered-target-command'),
                    'no_direct_target_command': count('no-direct-target-command'),
                    'targets_with_acceptance_shared_sources': sum(bool(row['acceptance_shared_source_references']) for row in targets),
                    'unregistered_targets_selecting_full_replay': sum(row['direct_mode'] == 'no-direct-target-command' and row['plan_for_target_source_change']['acceptance'] == list(replay.GROUPS) and row['plan_for_target_source_change']['witnesses'] == list(replay.witness.SUITES) for row in targets)},
        'acceptance_driver_sources': drivers, 'acceptance_source_references': references,
        'targets': targets, 'lib_bin_harnesses': harnesses,
        'source_sha256': {relative(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(READ)},
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument('--write', action='store_true', help='write a NEW snapshot only')
    action.add_argument('--check', action='store_true', help='compare current source with the frozen snapshot')
    parser.add_argument('--output', type=Path, default=HERE / 'inventory.v21.json')
    args = parser.parse_args()
    if args.check:
        source_commit = json.loads(args.output.read_text())['source_commit']
    else:
        source_commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    artifact = inventory(source_commit)
    rendered = json.dumps(artifact, indent=2, ensure_ascii=False) + '\n'
    if args.write:
        with args.output.open('x') as output:
            output.write(rendered)
    else:
        assert args.output.read_text() == rendered, 'inventory drift: inspect source/entry changes before a new snapshot'
    print(json.dumps(artifact['summary']))


if __name__ == '__main__':
    main()
