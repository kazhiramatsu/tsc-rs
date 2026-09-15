#!/usr/bin/env python3
"""Classify A6-41-SUPER native captures against their frozen upstream tuples.

usage: decorator-super-analyze.py <captures-dir> [--diff N] [--filter substr] [--json out]

Each capture holds the complete native command tuple ("actual") beside the
frozen TypeScript observation ("expected"). The first difference of every
case is classified (javascript / source-map / declaration / declaration-map /
reported-diagnostics / emit-diagnostics / exit / writes) so a run can be
read per family without opening captures.
"""
from pathlib import Path
import base64, difflib, json, sys
from collections import Counter, defaultdict

def text(write):
    return base64.b64decode(write['callback_utf8_base64']).decode('utf-8', 'replace')

def classify(actual, expected):
    kinds = []
    ew = {w['path']: w for w in expected['writes']}
    aw = {w['path']: w for w in (actual or {}).get('writes', [])}
    if actual is None:
        return ['native-error']
    for path, w in ew.items():
        a = aw.get(path)
        if a is None:
            kinds.append('missing-write:' + path.rsplit('.', 1)[-1]); continue
        if text(a) != text(w):
            kinds.append({'javascript': 'javascript', 'source-map': 'source-map', 'declaration': 'declaration', 'declaration-map': 'declaration-map'}.get(w['kind'], w['kind']))
    if len(aw) != len(ew):
        kinds.append('write-count')
    if actual['reported_diagnostics'] != expected['reported_diagnostics']:
        kinds.append('reported-diagnostics')
    if actual['emit_result']['diagnostics'] != expected['emit_result']['diagnostics']:
        kinds.append('emit-diagnostics')
    if actual['emit_result']['source_maps'] != expected['emit_result']['source_maps']:
        kinds.append('emit-result-maps')
    if actual['exit_code'] != expected['exit_code']:
        kinds.append('exit')
    if actual['status_writes'] != expected['status_writes']:
        kinds.append('status')
    return kinds or ['exact']

def main():
    args = sys.argv[1:]
    captures_dir = Path(args[0])
    diff_n = int(args[args.index('--diff') + 1]) if '--diff' in args else 0
    flt = args[args.index('--filter') + 1] if '--filter' in args else None
    out = args[args.index('--json') + 1] if '--json' in args else None
    by_case = {}
    for f in sorted(captures_dir.glob('*.json')):
        c = json.loads(f.read_text())
        by_case.setdefault(c['case_id'], c)  # first capture per case
    rows = {}
    per_family = defaultdict(Counter)
    per_combo = defaultdict(Counter)
    shown = 0
    for case_id, c in sorted(by_case.items()):
        if flt and flt not in case_id:
            continue
        kinds = classify(c['actual'], c['expected'])
        rows[case_id] = kinds
        parts = case_id.split('/')
        family = parts[3] if len(parts) > 3 else '?'
        label = '+'.join(kinds)
        per_family[family][label] += 1
        per_combo[parts[1] + '/' + parts[2]][label] += 1
        if diff_n and shown < diff_n and kinds != ['exact'] and c['actual'] is not None:
            ew = {w['path']: w for w in c['expected']['writes']}
            aw = {w['path']: w for w in c['actual']['writes']}
            for path in ew:
                if path.endswith('.js') and path in aw and text(aw[path]) != text(ew[path]):
                    print('=' * 20, case_id, kinds)
                    for line in difflib.unified_diff(text(ew[path]).splitlines(), text(aw[path]).splitlines(), 'expected', 'actual', lineterm='', n=1):
                        print(line)
                    shown += 1
                    break
            else:
                print('=' * 20, case_id, kinds, '(no JS text difference)')
                shown += 1
    total = len(rows)
    exact = sum(1 for k in rows.values() if k == ['exact'])
    print(f'cases={total} exact={exact} differing={total - exact}')
    for family, counter in sorted(per_family.items()):
        print(f'  {family:14s}', dict(counter))
    for combo, counter in sorted(per_combo.items()):
        print(f'  {combo:14s}', dict(counter))
    if out:
        Path(out).write_text(json.dumps({'captures_dir': str(captures_dir), 'cases': total, 'exact': exact, 'rows': rows}, indent=1) + '\n')

if __name__ == '__main__':
    main()
