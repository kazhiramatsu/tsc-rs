"""Read-only detailed classification of generated JSON differences after the walk.
No generated result from this report qualifies output or approves a delta.
"""
from pathlib import Path
from collections import Counter
import hashlib
import json
import subprocess

ROOT = Path('/Users/hiramatsu/dev/tsc-rs-emitter-final-variable-producer-prep')
OUT = Path('/tmp/emitter-final-generated-leaves-r597.json')
def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)
def signature(value):
    raw = json.dumps(value, ensure_ascii=True, allow_nan=False, separators=(',', ':')).encode()
    return {'type': type(value).__name__, 'bytes': len(raw), 'sha256': hashlib.sha256(raw).hexdigest(),
            'preview': raw.decode()[:220]}
def differences(old, new, path=()):
    if type(old) is not type(new):
        yield {'path': list(path), 'kind': 'type', 'before': signature(old), 'after': signature(new)}
    elif isinstance(old, dict):
        if list(old) != list(new):
            yield {'path': list(path), 'kind': 'object_keys', 'before': list(old), 'after': list(new)}
        for key in old.keys() | new.keys():
            if key not in old or key not in new:
                yield {'path': list(path + (key,)), 'kind': 'membership', 'before_present': key in old, 'after_present': key in new}
            else:
                yield from differences(old[key], new[key], path + (key,))
    elif isinstance(old, list):
        if len(old) != len(new):
            yield {'path': list(path), 'kind': 'array_length', 'before': len(old), 'after': len(new)}
        for index, (a, b) in enumerate(zip(old, new)):
            yield from differences(a, b, path + (index,))
    elif old != new:
        yield {'path': list(path), 'kind': 'value', 'before': signature(old), 'after': signature(new)}

def main():
    review = json.loads(Path('/tmp/emitter-final-walk-review-r597.json').read_text())
    assert review['walk_receipt_success']
    assert git('rev-parse', 'HEAD').decode().strip() == review['walk_source_head']
    assert not OUT.exists()
    rows = []
    for row in review['changes']:
        name = row['path']
        if not name.endswith('.json'):
            continue
        before, after = git('show', 'HEAD:' + name), (ROOT / name).read_bytes()
        assert hashlib.sha256(before).hexdigest() == row['before_sha256']
        assert hashlib.sha256(after).hexdigest() == row['after_sha256']
        delta = list(differences(json.loads(before), json.loads(after)))
        groups = Counter('/'.join('*' if isinstance(x, int) else str(x) for x in d['path']) for d in delta)
        rows.append({'path': name, 'leaf_count': len(delta), 'grouped_paths': dict(sorted(groups.items())), 'differences': delta})
    result = {'source_head': review['walk_source_head'], 'qualification_claim': False, 'changes': rows}
    OUT.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'output': str(OUT), 'files': len(rows), 'changed_leaves': sum(r['leaf_count'] for r in rows)}))

if __name__ == '__main__':
    main()
