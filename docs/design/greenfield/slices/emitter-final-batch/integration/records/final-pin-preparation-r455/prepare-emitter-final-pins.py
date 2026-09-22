"""Prepare source-reference-only changes. Review the proposal before copying it.

This does not qualify evidence, alter dispositions, or run the oracle walk.
The output is outside the worktree. Re-run at final source; never reuse stale bytes.
"""
import argparse
import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tree', type=Path, required=True)
    parser.add_argument('--head', required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    tree = args.tree.resolve()
    head = subprocess.check_output(['git', '-C', str(tree), 'rev-parse', 'HEAD'], text=True).strip()
    assert head == args.head, 'source HEAD changed'
    assert not subprocess.check_output(['git', '-C', str(tree), 'diff', 'HEAD', '--name-only']).strip(), 'commit source first'
    args.out.mkdir(parents=True, exist_ok=False)
    prepared, changes = {}, []

    def current(path):
        assert not Path(path).is_absolute() and '..' not in Path(path).parts
        return prepared.get(path, (tree / path).read_bytes())

    def replace_reference(owner, ref):
        assert set(ref) == {'path', 'sha256'}
        digest = sha(current(ref['path']))
        if digest != ref['sha256']:
            changes.append({'owner': owner, 'path': ref['path'], 'before': ref['sha256'], 'after': digest})
            ref['sha256'] = digest

    paths = ['ratchets/fuzz-oracle-deviations.v1.json', 'ratchets/fuzz-domain.v1.toml',
             'ratchets/fuzz-preflight.v1.json', '.github/ci/qualification-policy.v2.json']
    for path in paths:
        raw = (tree / path).read_bytes()
        is_toml = path.endswith('.toml')
        old = tomllib.loads(raw.decode()) if is_toml else json.loads(raw)
        new = copy.deepcopy(old)
        if path.startswith('ratchets/fuzz-'):
            assert old['status'] == 'draft', 'never restamp qualified fuzz evidence'
            for ref in new['source_references']:
                replace_reference(path, ref)
            # Every check, deviation, membership, evidence string and disposition stays exact.
            assert {k:v for k,v in old.items() if k != 'source_references'} == {
                k:v for k,v in new.items() if k != 'source_references'}
            assert [r['path'] for r in old['source_references']] == [r['path'] for r in new['source_references']]
        else:
            for key in ['rust_source_sha256', 'execution_source_sha256']:
                for source, old_hash in old['hosted_acceptance'][key].items():
                    digest = sha(current(source))
                    if digest != old_hash:
                        changes.append({'owner': path, 'field': key, 'path': source, 'before': old_hash, 'after': digest})
                        new['hosted_acceptance'][key][source] = digest
            restored = copy.deepcopy(new)
            for key in ['rust_source_sha256', 'execution_source_sha256']:
                assert set(new['hosted_acceptance'][key]) == set(old['hosted_acceptance'][key])
                restored['hosted_acceptance'][key] = old['hosted_acceptance'][key]
            assert restored == old, 'policy changed outside existing source hashes'
        if is_toml:
            rendered = raw.decode()
            for before, after in zip(old['source_references'], new['source_references']):
                pattern = '(\\[\\[source_references\\]\\]\\npath = "' + re.escape(before['path']) + '"\\nsha256 = ")' + before['sha256'] + '("\\n)'
                rendered, count = re.subn(pattern, lambda m: m[1] + after['sha256'] + m[2], rendered)
                assert count == 1, before['path']
            assert tomllib.loads(rendered) == new
            data = rendered.encode()
        else:
            # Preserve formatting, requiring each old complete source reference to be unique.
            data = raw
            for change in [c for c in changes if c['owner'] == path]:
                if 'field' in change:
                    pattern = (json.dumps(change['path']) + ': "' + change['before'] + '"').encode()
                    replacement = (json.dumps(change['path']) + ': "' + change['after'] + '"').encode()
                else:
                    pattern = re.compile(rb'("path": "' + re.escape(change['path'].encode()) + rb'",\s*"sha256": ")' + change['before'].encode() + rb'(")')
                    data, count = pattern.subn(lambda m: m[1] + change['after'].encode() + m[2], data)
                    assert count == 1, change['path']
                    continue
                assert data.count(pattern) == 1, change['path']
                data = data.replace(pattern, replacement)
            assert json.loads(data) == new
        prepared[path] = data
        destination = args.out / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(data)
    report = {'source_head': head, 'qualification_claim': False, 'changes': changes,
              'files': [{'path': p, 'before': sha((tree / p).read_bytes()), 'after': sha(d)} for p,d in prepared.items()]}
    (args.out / 'proposal.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({'source_head': head, 'reference_changes': len(changes), 'output': str(args.out)}))


if __name__ == '__main__':
    main()
