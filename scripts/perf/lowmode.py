#!/usr/bin/env python3
"""Summarize a serial.py JSONL record as low-mode instruction deltas per binary and project.

usage: lowmode.py RECORD.jsonl [--base LABEL]
The low mode of a binary's rounds is the median of the lower half of its instruction counts: VS Code's single-thread
counts are bimodal run to run (two modes about 0.75% apart, independent of the binary), so medians of few rounds
mislead for sub-1% deltas. Use 9 rounds (measure-patches.sh ROUNDS=9) and read this table.
"""
import collections
import json
import sys


def low(values):
    values = sorted(values)
    half = values[: max(1, (len(values) + 1) // 2)]
    return half[len(half) // 2]


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    base_label = sys.argv[sys.argv.index('--base') + 1] if '--base' in sys.argv else 'base'
    by = collections.defaultdict(list)
    order = []
    for line in open(path):
        row = json.loads(line)
        key = (row['case'], row['label'])
        if row['label'] not in order:
            order.append(row['label'])
        by[key].append(row['instr'])
    cases = sorted({case for case, _ in by}, key=lambda c: [k for k in by if k[0] == c][0][0])
    cases = list(dict.fromkeys(case for case, _ in by))
    labels = [base_label] + [label for label in order if label != base_label]
    width = max(len(label) for label in labels) + 2
    print(f"{'label':{width}}" + ''.join(f"{case.split('/')[0]:>11}" for case in cases))
    base = {case: low(by[(case, base_label)]) for case in cases}
    print(f"{base_label + ' (low-mode G)':{width}}" + ''.join(f"{base[case] / 1e9:11.3f}" for case in cases))
    for label in labels[1:]:
        cells = []
        for case in cases:
            values = by.get((case, label))
            cells.append(f"{(low(values) - base[case]) / base[case] * 100:+10.2f}%" if values else f"{'n/a':>11}")
        print(f"{label:{width}}" + ''.join(cells))


if __name__ == '__main__':
    main()
