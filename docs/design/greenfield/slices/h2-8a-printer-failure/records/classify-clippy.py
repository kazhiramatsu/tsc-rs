#!/usr/bin/env python3
"""Classify clippy warnings: inside candidate diff hunks (new), in new files (new), or elsewhere (inherited)."""
import re, subprocess, sys, collections
log = open(sys.argv[1], encoding='utf-8', errors='replace').read()
root = sys.argv[2]
hunks = collections.defaultdict(list)
diff = subprocess.run(['git', '-C', root, 'diff', '-U0', '--', 'crates/emitter/src/printer.rs', 'crates/emitter/src/printer/bundle.rs'], capture_output=True, text=True).stdout
current = None
for line in diff.splitlines():
    if line.startswith('+++ b/'):
        current = line[6:]
    m = re.match(r'@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@', line)
    if m and current:
        start = int(m.group(1)); count = int(m.group(2) or '1')
        hunks[current].append((start, start + max(count, 1) - 1))
new_files = {'crates/emitter/tests/printer_failure_contract.rs'}
blocks = re.split(r'\n(?=warning: |error: )', log)
rows = []
for block in blocks:
    head = block.split('\n', 1)[0]
    if not (head.startswith('warning: ') or head.startswith('error: ')):
        continue
    if head.startswith('warning: `tsc-rs-emitter`') or 'generated' in head and 'warning' in head and head.endswith('warnings'):
        continue
    loc = re.search(r'--> ([^:]+):(\d+):(\d+)', block)
    if not loc:
        continue
    path, line = loc.group(1), int(loc.group(2))
    if path in new_files:
        cls = 'new-file'
    elif path in hunks and any(a <= line <= b for a, b in hunks[path]):
        cls = 'in-candidate-hunk'
    else:
        cls = 'inherited'
    rows.append((cls, path, line, head))
counts = collections.Counter(r[0] for r in rows)
print('counts:', dict(counts))
for cls in ('new-file', 'in-candidate-hunk'):
    for r in rows:
        if r[0] == cls:
            print(f'{cls}: {r[1]}:{r[2]} {r[3]}')
print('inherited by file:', collections.Counter(r[1] for r in rows if r[0] == 'inherited'))
