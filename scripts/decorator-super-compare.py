#!/usr/bin/env python3
"""Classify a run's differing cases against an earlier run of the same fixture.

usage: decorator-super-compare.py <before-captures> <after-captures> [--show N]

For every case that still differs after, the JavaScript hunks of after-vs-expected
are compared with before-vs-expected: a hunk present in both runs is
`pre-existing`, a hunk only in after is `introduced`, cases exact before but not
after are `regressed`. Map-only differences are reported as such.
"""
from pathlib import Path
import base64, difflib, json, sys
from collections import Counter

def load(captures):
    by = {}
    for f in sorted(Path(captures).glob('*.json')):
        c = json.loads(f.read_text()); by.setdefault(c['case_id'], c)
    return by

def js(capture, which):
    if capture.get(which) is None: return None
    for w in capture[which]['writes']:
        if w['path'].endswith('.js'): return base64.b64decode(w['callback_utf8_base64']).decode('utf-8', 'replace')
    return None

def hunks(expected, actual):
    if expected is None or actual is None: return None
    out = set(); cur = []
    for line in difflib.unified_diff(expected.splitlines(), actual.splitlines(), lineterm='', n=0):
        if line.startswith('@@'):
            if cur: out.add(tuple(cur)); cur = []
        elif line[:1] in '+-' and not line.startswith('+++') and not line.startswith('---'):
            cur.append(line)
    if cur: out.add(tuple(cur))
    return out

def exact(capture):
    a, e = capture.get('actual'), capture['expected']
    if a is None: return False
    aw = {w['path']: w['callback_utf8_base64'] for w in a['writes']}; ew = {w['path']: w['callback_utf8_base64'] for w in e['writes']}
    return aw == ew and a['reported_diagnostics'] == e['reported_diagnostics'] and a['emit_result'] == e['emit_result'] and a['exit_code'] == e['exit_code'] and a['status_writes'] == e['status_writes']

def main():
    before, after = load(sys.argv[1]), load(sys.argv[2])
    show = int(sys.argv[sys.argv.index('--show') + 1]) if '--show' in sys.argv else 0
    counter = Counter(); rows = []
    for case_id, a in sorted(after.items()):
        if exact(a): counter['exact'] += 1; continue
        b = before.get(case_id)
        if b is None: counter['no-before'] += 1; rows.append((case_id, 'no-before', None)); continue
        if exact(b): counter['regressed'] += 1; rows.append((case_id, 'regressed', None)); continue
        ah = hunks(js(a, 'expected'), js(a, 'actual')); bh = hunks(js(b, 'expected'), js(b, 'actual'))
        if not ah:
            kind = 'map-only-after'
            counter['map-only' if not bh else 'map-only-after(js-differed-before)'] += 1
        else:
            introduced = ah - (bh or set())
            kind = 'introduced' if introduced else 'pre-existing'
            counter[kind] += 1
        rows.append((case_id, kind, ah))
    print(dict(counter))
    shown = 0
    for case_id, kind, ah in rows:
        print(f'{kind:12s} {case_id}')
        if show and shown < show and ah:
            for h in list(ah)[:3]: print('    ' + ' | '.join(h)[:400])
            shown += 1

main()
