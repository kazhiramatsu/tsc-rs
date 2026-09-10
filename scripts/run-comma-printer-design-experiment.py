#!/usr/bin/env python3
"""Compare the staged comma printer in the isolated A40 design workspace.

This produces research evidence, never production admission or qualification.
Completed attempts and the production implementation are immutable.
"""
from pathlib import Path
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
CANDIDATE = Path('docs/design/greenfield/slices/h2-8a-retained-lexical-owners.candidate-v2.rs.txt')
PRODUCTION = Path('crates/emitter/src/builtins/class_fields.rs')
BASELINE = Path('ratchets/h2-8a-retained-lexical-owners-before.v3.json')
PRODUCTION_SHA = '26de511b53ea232b073235a4507f703ae78cd4881c12bfea720f71f932dc03e7'
CANDIDATE_SHA = 'ab3363962f79229d4a10433f0c88126912efea062f61347e3a94eb38ccc39573'
BASELINE_SHA = 'b16b64964c0e8cd78d652d38efd9b303906a35147df73e32a67fc35c1ed0bed8'
PRINTER = Path('crates/emitter/src/printer.rs')
PATCH = Path('docs/design/greenfield/slices/h2-8a-comma-printer.candidate.patch')
PRINTER_SHA = '8951dc94e07df2cfdca2f41a7b82e4c9ceeae75d6d5ff60ae5dabb752db53398'
PATCH_SHA = 'f8a30aca24079b013b1396af6ffb769eabe8509af700e4f63f1fdf4f2ce8b90c'
FACTORY = Path('crates/emitter/src/factory.rs')
FACTORY_PATCH = Path('docs/design/greenfield/slices/h2-8a-comma-argument-factory.candidate.patch')
FACTORY_SHA = '4c0ade2cd1a17a83bb9af5c0c53628a4f88017ef241ed6bb3e27a42aff1094f0'
FACTORY_PATCH_SHA = '4c4ba5f1406ce9b36ae076423a4422a72e91904f6f35edc86b34aa375e9c8039'
LIST_OWNER_PATCH = Path('docs/design/greenfield/slices/h2-8a-list-intervening-printer.candidate-v18.patch')
LIST_OWNER_PATCH_SHA = '839f72ea3535695548dacfd13d51d92dc47c1a86138cb15145cb8f62c8e51d93'
WRITER = Path('crates/emitter/src/writer.rs')
WRITER_SHA = '0e3c1168e6251a7c42a5d9a811c0e4debfaa85398cafb9c512116a85f6bcecb6'
BUNDLE_PRINTER = Path('crates/emitter/src/printer/bundle.rs')
BUNDLE_PRINTER_SHA = 'b948d3825de0cb9e558094639b4a0a8f0558a6d2ccc35635e203c6f507d50d12'

UTF16_TEST_PATCH = Path('docs/design/greenfield/slices/h2-8a-utf16-writer-tests.candidate-v4.patch')
UTF16_TEST_PATCH_SHA = 'a6b26210edc32ad194959038a028b913a48c9b7e2f164a669a514ba30bac71fc'
UTF16_TEST_BASES = {'crates/emitter/tests/integration/declaration_printer_reprint_contract.rs': '9bc851af71f40a2fb2e70c0f5901f2b306214db830ced7d83cfbac79dfedfd9b', 'crates/emitter/tests/integration/factory_transform_contract.rs': '250671479a6fa24fdbeb6a49afcefa0a7e2624fbe415d933d74fc5ebd3820e33', 'crates/emitter/tests/literal_parent_provenance_contract.rs': '1146af1be6cdaac6fe1003200091f5763c17e28c1594df4fc4667fe4ad1c26cd', 'crates/emitter/tests/string_literal_identifier_source_contract.rs': '52566df0e08fd206cd16a3d874c1e6e053d455b1175d4ca2e6fbb30574da2ca9', 'crates/emitter/tests/unit/factory_seams/tests.rs': '4e8219824921f059ed16cedb1dc740a56ea6f1c98250c5d426ec1636cb10cd66', 'crates/emitter/tests/utf16_literal_escaping_contract.rs': '00254a6e7b6ca7b8422c2e9b53c25e2d1b9367f2bbdfb58909a9a5f6ef76ecf7'}
UTF16_TEST_ADDITIONS = [Path('crates/emitter/tests/utf16_writer_contract.rs'), Path('crates/emitter/tests/literal_value_provenance_contract.rs')]

METADATA = Path('crates/emitter/src/metadata.rs')
METADATA_SHA = '7f73ec168773329fbaff08795de32b4504833c3d6dd8583badced16016a29933'
LITERAL_OBSERVATION_PATCH = Path('docs/design/greenfield/slices/h2-8a-literal-property-observation.candidate.patch')
LITERAL_OBSERVATION_PATCH_SHA = '51d1a3bec8b8c7c517d376dbb8a2e32f0a24d21a62e3aa25143ac95e794fddef'

RECEIVER_DESIGN = Path('docs/design/greenfield/slices/h2-8a-decorator-receiver-frames.md')
STANDARD_DECORATORS = Path('crates/emitter/src/builtins/standard_decorators.rs')
STANDARD_DECORATORS_SHA = '042d017e951554a104adf938c138e28af63672ba6ff91b69d671b0c88b40b659'
DECORATOR_ROUTING_TEST_PATCH = Path('docs/design/greenfield/slices/h2-8a-decorator-static-accessor-test.candidate-v2.patch')
DECORATOR_ROUTING_TEST_PATCH_SHA = '04ac9fe5b1ea2ee62c506e5fc711f21d4ecf73cdc5ee84c536a01c0d59ab5e67'
DECORATOR_ROUTING_TEST = Path('crates/emitter/tests/integration/active_transform_contract.rs')
DECORATOR_ROUTING_TEST_BASE_SHA = '94d65f57a35dd2e6baae7c83d8174f2568c758352f52a11fc7ffed08cfb6a657'
DECORATOR_ROUTING_FIXTURE = Path('crates/emitter/tests/fixtures/decorator-static-accessor-routing.json')
DECORATOR_ROUTING_FIXTURE_SHA = '062dfb6eccb239bc0a0edf9f309a7cb8bc1682690c68a4088eb44dbeaade387f'


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write_json(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def main():
    attempt = int(sys.argv[1])
    assert attempt > 0
    selection = sys.argv[2]
    assert selection in ['direct', 'factory-direct', 'emitter', 'all', 'edges', 'comma-factory', 'literal-neighbors', 'context', 'additional']
    flags = sys.argv[3:]
    assert len(flags) == len(set(flags)) and set(flags) <= {'--factory', '--list-owner'}
    with_factory = '--factory' in flags
    with_list_owner = '--list-owner' in flags
    assert not with_list_owner or with_factory
    assert selection != 'factory-direct' or with_list_owner
    prefix = ROOT / f'target/h2-8a-comma-printer-design-experiment-{attempt}'
    pre_path = prefix.with_suffix('.pre.json')
    log_path = prefix.with_suffix('.log')
    exit_path = prefix.with_suffix('.exit.json')
    assert not any(p.exists() for p in [pre_path, log_path, exit_path]), 'poll the existing attempt; never restart it'
    assert sha(ROOT / PRODUCTION) == PRODUCTION_SHA
    assert sha(ROOT / CANDIDATE) == CANDIDATE_SHA
    assert sha(ROOT / BASELINE) == BASELINE_SHA
    assert sha(ROOT / PRINTER) == PRINTER_SHA
    assert sha(ROOT / PATCH) == PATCH_SHA
    if with_list_owner:
        assert sha(ROOT / LIST_OWNER_PATCH) == LIST_OWNER_PATCH_SHA
        assert sha(ROOT / DECORATOR_ROUTING_TEST_PATCH) == DECORATOR_ROUTING_TEST_PATCH_SHA
        assert sha(ROOT / DECORATOR_ROUTING_TEST) == DECORATOR_ROUTING_TEST_BASE_SHA
        assert sha(ROOT / DECORATOR_ROUTING_FIXTURE) == DECORATOR_ROUTING_FIXTURE_SHA
        assert sha(ROOT / STANDARD_DECORATORS) == STANDARD_DECORATORS_SHA
        assert sha(ROOT / BUNDLE_PRINTER) == BUNDLE_PRINTER_SHA
        assert sha(ROOT / WRITER) == WRITER_SHA
        assert sha(ROOT / METADATA) == METADATA_SHA
        assert sha(ROOT / LITERAL_OBSERVATION_PATCH) == LITERAL_OBSERVATION_PATCH_SHA
        assert sha(ROOT / UTF16_TEST_PATCH) == UTF16_TEST_PATCH_SHA
        for path, expected in UTF16_TEST_BASES.items():
            assert sha(ROOT / path) == expected
        assert all(not (ROOT / path).exists() for path in UTF16_TEST_ADDITIONS)
    if with_factory:
        assert sha(ROOT / FACTORY) == FACTORY_SHA
        assert sha(ROOT / FACTORY_PATCH) == FACTORY_PATCH_SHA
    workspace = ROOT / 'target/h2-8a-retained-lexical-design-workspace'
    assert workspace.is_dir(), 'use the existing isolated typecheck workspace'
    source_paths = sorted(p.relative_to(ROOT) for p in (ROOT / 'crates').rglob('*') if p.is_file())
    source_paths += [Path(p) for p in ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']]
    source_paths += sorted(p.relative_to(ROOT) for p in (ROOT / '.cargo').rglob('*') if p.is_file())
    # The contracts target compiles sibling tests even when only one test is
    # selected. Copy their root-relative include_bytes!/include_str! inputs as
    # well; merely copying crates is sufficient only for the library check.
    include_pattern = re.compile(
        r'include_(?:str|bytes)!\(\s*concat!\(\s*env!\("CARGO_MANIFEST_DIR"\),'
        r'\s*"/\.\./\.\./([^"\n]+)"\s*,?\s*\)\s*\)', re.MULTILINE)
    includes = {}
    for source in source_paths:
        if source.suffix == '.rs':
            for spelling in include_pattern.findall((ROOT / source).read_text()):
                included = Path(spelling)
                assert not included.is_absolute() and '..' not in included.parts
                assert (ROOT / included).is_file(), spelling
                includes.setdefault(spelling, []).append(str(source))
    source_paths = sorted(set(source_paths) | {Path(p) for p in includes})
    root_source_paths = list(source_paths)
    for relative in source_paths:
        original = ROOT / (CANDIDATE if relative == PRODUCTION else relative)
        destination = workspace / relative
        if not destination.exists() or sha(destination) != sha(original):
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(original, destination)
    subprocess.run(['git', 'apply', '--check', str(ROOT / PATCH)], cwd=workspace, check=True)
    subprocess.run(['git', 'apply', str(ROOT / PATCH)], cwd=workspace, check=True)
    if with_factory:
        subprocess.run(['git', 'apply', '--check', str(ROOT / FACTORY_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', str(ROOT / FACTORY_PATCH)], cwd=workspace, check=True)
    if with_list_owner:
        subprocess.run(['git', 'apply', '--check', str(ROOT / LITERAL_OBSERVATION_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', str(ROOT / LITERAL_OBSERVATION_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', '--check', str(ROOT / LIST_OWNER_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', str(ROOT / LIST_OWNER_PATCH)], cwd=workspace, check=True)
        # Remove only the declared generated test left by an earlier isolated
        # attempt. Root has no corresponding file or candidate-only API.
        for relative in UTF16_TEST_ADDITIONS:
            (workspace / relative).unlink(missing_ok=True)
        subprocess.run(['git', 'apply', '--check', str(ROOT / UTF16_TEST_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', str(ROOT / UTF16_TEST_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', '--check', str(ROOT / DECORATOR_ROUTING_TEST_PATCH)], cwd=workspace, check=True)
        subprocess.run(['git', 'apply', str(ROOT / DECORATOR_ROUTING_TEST_PATCH)], cwd=workspace, check=True)
        source_paths = sorted(set(source_paths) | set(UTF16_TEST_ADDITIONS))
    inputs = [{'path': str(p), 'sha256': sha(workspace / p)} for p in source_paths]
    production_inputs = [{'path': str(p), 'sha256': sha(ROOT / p)} for p in root_source_paths]
    previous_pre = json.loads((ROOT / 'target/h2-8a-retained-comma-native-before-1-prelaunch.json').read_text())
    vendor_inputs = [r for r in previous_pre['inputs'] if r['path'].startswith('vendor/')]
    for row in vendor_inputs:
        assert sha(ROOT / row['path']) == row['sha256'], row['path']
    archive = Path(tempfile.mkdtemp(prefix='tsc-rs-comma-printer-design-experiment-'))
    with tarfile.open(archive / 'source-and-inputs.tar.gz', 'w:gz') as tar:
        for row in inputs:
            tar.add(workspace / row['path'], arcname=row['path'])
        for row in vendor_inputs:
            tar.add(ROOT / row['path'], arcname=row['path'])
        for path in [CANDIDATE, BASELINE, PATCH, Path(__file__).relative_to(ROOT),
                     Path('docs/design/greenfield/slices/h2-8a-retained-lexical-owners.md')]:
            tar.add(ROOT / path, arcname=str(path))
        if with_factory:
            tar.add(ROOT / FACTORY_PATCH, arcname=str(FACTORY_PATCH))
        if with_list_owner:
            tar.add(ROOT / LIST_OWNER_PATCH, arcname=str(LIST_OWNER_PATCH))
            tar.add(ROOT / RECEIVER_DESIGN, arcname=str(RECEIVER_DESIGN))
            tar.add(ROOT / UTF16_TEST_PATCH, arcname=str(UTF16_TEST_PATCH))
            tar.add(ROOT / LITERAL_OBSERVATION_PATCH, arcname=str(LITERAL_OBSERVATION_PATCH))
            tar.add(ROOT / DECORATOR_ROUTING_TEST_PATCH, arcname=str(DECORATOR_ROUTING_TEST_PATCH))
    target = ROOT / 'target/h2-8a-retained-lexical-design-artifacts'
    command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
               'cargo', 'test', '--offline', '-p', 'tsc-rs-compiler', '--test', 'contracts', '--',
               'retained_accessor_owners_match_complete_typescript_observations',
               '--nocapture', '--test-threads=1']
    if selection in ['direct', 'factory-direct']:
        command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
                   'cargo', 'test', '--offline', '--no-fail-fast', '-p', 'tsc-rs-emitter', '--test', 'comma_list_printer_contract',
                   '--', '--nocapture', '--test-threads=1']
        if selection == 'factory-direct':
            command[command.index('--'):command.index('--')] = [
                '--test', 'comma_argument_factory_contract', '--test', 'mapped_type_members_contract',
                '--test', 'list_format_flags_contract', '--test', 'import_type_attributes_contract',
                '--test', 'emit_pipeline_phases_contract', '--test', 'literal_parent_provenance_contract',
                '--test', 'utf16_literal_escaping_contract', '--test', 'utf16_writer_contract', '--test', 'literal_value_provenance_contract']
    elif selection == 'literal-neighbors':
        command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
                   'cargo', 'test', '--offline', '-p', 'tsc-rs-emitter',
                   '--test', 'string_literal_identifier_source_contract',
                   '--', '--nocapture', '--test-threads=1']
    elif selection == 'emitter':
        command = ['/usr/sbin/taskpolicy', '-b', '/usr/bin/nice', '-n', '15',
                   'cargo', 'test', '--offline', '--no-fail-fast', '-p', 'tsc-rs-emitter',
                   '--lib', '--test', 'contracts', '--', '--test-threads=1']
    environment = {'CARGO_BUILD_JOBS': '2', 'CARGO_TARGET_DIR': str(target),
                   'TSC_RS_RETAINED_ACCESSOR_CASE_SET': selection,
                   'TSC_RS_H2_8A_CAPTURE_WRITES_DIR': str(archive / 'captures'),
                   'TSC_RS_H2_8A_FAILURE_DIR': str(archive / 'failures')}
    pre = {'version': 1, 'attempt': attempt, 'status': 'isolated design experiment; no production edit, admission or qualification',
           'base': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
           'workspace': str(workspace), 'archive': str(archive), 'argv': command,
           'environment': environment, 'inputs': inputs, 'vendor_inputs': vendor_inputs,
           'root_relative_include_consumers': includes,
           'production_inputs': production_inputs, 'candidate_sha256': CANDIDATE_SHA,
           'selection': selection, 'printer_patch_sha256': PATCH_SHA,
           'printer_base_sha256': PRINTER_SHA,
           'factory_patch': {'path': str(FACTORY_PATCH), 'sha256': FACTORY_PATCH_SHA,
                             'base_sha256': FACTORY_SHA} if with_factory else None,
           'list_owner_patch': {'path': str(LIST_OWNER_PATCH), 'sha256': LIST_OWNER_PATCH_SHA,
                                'writer_base_sha256': WRITER_SHA} if with_list_owner else None,
           'literal_property_observation_patch': {'path': str(LITERAL_OBSERVATION_PATCH),
                                                  'sha256': LITERAL_OBSERVATION_PATCH_SHA,
                                                  'root_base_sha256': METADATA_SHA,
                                                  'purpose': 'read-only getter visibility; no field or transition changes'} if with_list_owner else None,
           'utf16_test_patch': {'path': str(UTF16_TEST_PATCH), 'sha256': UTF16_TEST_PATCH_SHA,
                                'root_bases': UTF16_TEST_BASES,
                                'root_absent': [str(p) for p in UTF16_TEST_ADDITIONS]} if with_list_owner else None,
           'decorator_routing_test_patch': {'path': str(DECORATOR_ROUTING_TEST_PATCH),
                                            'sha256': DECORATOR_ROUTING_TEST_PATCH_SHA,
                                            'root_test': str(DECORATOR_ROUTING_TEST),
                                            'root_base_sha256': DECORATOR_ROUTING_TEST_BASE_SHA,
                                            'fixture': str(DECORATOR_ROUTING_FIXTURE),
                                            'fixture_sha256': DECORATOR_ROUTING_FIXTURE_SHA} if with_list_owner else None,
           'baseline': {'path': str(BASELINE), 'sha256': BASELINE_SHA},
           'source_archive_sha256': sha(archive / 'source-and-inputs.tar.gz')}
    write_json(pre_path, pre)
    shutil.copy2(pre_path, archive / 'prelaunch.json')
    print(json.dumps({'event': 'launch', 'pre': str(pre_path), 'log': str(log_path), 'archive': str(archive)}), flush=True)
    started = time.monotonic()
    with log_path.open('x') as log:
        result = subprocess.run(command, cwd=workspace, env=dict(os.environ, **environment), stdout=log, stderr=subprocess.STDOUT)
    record = {'actual_exit': result.returncode, 'elapsed_seconds': time.monotonic() - started,
              'manifest_sha256': sha(pre_path), 'log_sha256': sha(log_path),
              'production_unchanged': all(sha(ROOT / row['path']) == row['sha256'] for row in production_inputs),
              'copied_inputs_unchanged': all(sha(workspace / row['path']) == row['sha256'] for row in inputs)}
    if with_list_owner:
        record['root_test_additions_still_absent'] = all(not (ROOT / p).exists() for p in UTF16_TEST_ADDITIONS)
        record['production_unchanged'] &= record['root_test_additions_still_absent']
    # Preserve the executed binary before allowing another Cargo job.
    binaries = re.findall(r'Running (?:tests/[^ ]+|unittests [^ ]+) \(([^\)]+)\)', log_path.read_text())
    record['binaries'] = []
    for index, spelling in enumerate(binaries):
        binary = Path(spelling)
        if not binary.is_absolute():
            binary = workspace / binary
        retained = archive / f'test-{index}.bin'
        shutil.copy2(binary, retained)
        record['binaries'].append({'path': str(binary), 'retained_path': str(retained),
                                   'sha256': sha(retained), 'size': retained.stat().st_size})
    write_json(exit_path, record)
    shutil.copy2(exit_path, archive / 'actual-exit.json')
    shutil.copy2(log_path, archive / 'run.log')
    print(json.dumps(record), flush=True)
    return result.returncode


if __name__ == '__main__':
    sys.exit(main())
