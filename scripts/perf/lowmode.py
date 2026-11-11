#!/usr/bin/env python3
"""Summarize a serial.py JSONL record as low-mode instruction deltas per binary and project.

usage: lowmode.py RECORD.jsonl [--base LABEL] [--gap PERCENT]

A project's one-thread instruction counts are multimodal run to run: on this machine the modes lie 0.35-1.7 % apart
(VS Code about 0.86 %, zod 1.7 %, next 1.1-1.5 %, playwright 1.7 %, vue 0.6-0.9 %, TypeScript 0.35 %), they do not
depend on the binary, and the rounds of a quiet window that land in one mode spread by at most 0.08 %. The low mode of
a binary's rounds is therefore the median of its LOWEST CLUSTER: the sorted counts split wherever two neighbours lie
more than --gap percent (default 0.15) of the lowest count apart, and the first cluster is the low mode. The second
table shows how many rounds landed in that cluster; a low cluster of fewer than 3 rounds is flagged (re-measure with
more rounds, e.g. 15). Use 9 rounds (measure-patches.sh ROUNDS=9) and read both tables. The earlier rule (median of
the lower half) misreported a neutral row by +1.5 % when 7 of 9 rounds landed in a high mode (zod/next, 2026-11-09).

When the record carries `cycles` (serial.py since 2026-11-11), a third table gives each binary's median cycles per
project against the base. Cycles are what the time is made of: a row can remove instructions that never stalled and
leave the time where it was, or remove stalls without removing an instruction. They spread by about 1.5 % run to run
and do not form the instruction counts' tight clusters, so the table is the plain median of all rounds; read a
difference under 1 % only from 15 or more rounds.
"""
import collections
import json
import statistics
import sys

MIN_LOW_ROUNDS = 3


def clusters(values, gap):
    """Sorted values split where neighbours differ by more than `gap` (a fraction) of the lowest value."""
    values = sorted(values)
    out = [[values[0]]]
    for previous, current in zip(values, values[1:]):
        if current - previous > gap * values[0]:
            out.append([current])
        else:
            out[-1].append(current)
    return out


def low(values, gap):
    """The low mode (median of the lowest cluster) and the size of that cluster."""
    first = clusters(values, gap)[0]
    return first[len(first) // 2], len(first)


def option(name, default, convert):
    return convert(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    base_label = option('--base', 'base', str)
    gap = option('--gap', 0.15, float) / 100
    by = collections.defaultdict(list)
    cycles = collections.defaultdict(list)
    order = []
    for line in open(path):
        if not line.strip():
            continue
        row = json.loads(line)
        if row['label'] not in order:
            order.append(row['label'])
        by[(row['case'], row['label'])].append(row['instr'])
        if row.get('cycles'):
            cycles[(row['case'], row['label'])].append(row['cycles'])
    cases = list(dict.fromkeys(case for case, _ in by))
    labels = [base_label] + [label for label in order if label != base_label]
    width = max(len(label) for label in labels) + 2
    modes = {key: low(values, gap) for key, values in by.items()}
    print(f"{'label':{width}}" + ''.join(f"{case.split('/')[0]:>11}" for case in cases))
    base = {case: modes[(case, base_label)][0] for case in cases}
    print(f"{base_label + ' (low-mode G)':{width}}" + ''.join(f"{base[case] / 1e9:11.3f}" for case in cases))
    for label in labels[1:]:
        cells = []
        for case in cases:
            mode = modes.get((case, label))
            cells.append(f"{(mode[0] - base[case]) / base[case] * 100:+10.2f}%" if mode else f"{'n/a':>11}")
        print(f"{label:{width}}" + ''.join(cells))
    print(f"\n{'low cluster / rounds':{width}}" + ''.join(f"{case.split('/')[0]:>11}" for case in cases))
    flagged = []
    for label in labels:
        cells = []
        for case in cases:
            mode = modes.get((case, label))
            if mode is None:
                cells.append(f"{'n/a':>11}")
                continue
            rounds = len(by[(case, label)])
            cells.append(f"{mode[1]:>7}/{rounds:<3}")
            if mode[1] < MIN_LOW_ROUNDS:
                flagged.append(f"{case} {label} ({mode[1]}/{rounds})")
        print(f"{label:{width}}" + ''.join(cells))
    for cell in flagged:
        print(f"WARN low cluster below {MIN_LOW_ROUNDS} rounds: {cell} -> re-measure with more rounds (gap {gap * 100:g} %)")
    if all(cycles.get((case, base_label)) for case in cases):
        medians = {key: statistics.median(values) for key, values in cycles.items()}
        print(f"\n{'cycles, median of all rounds':{width}}" + ''.join(f"{case.split('/')[0]:>11}" for case in cases))
        print(f"{base_label + ' (G)':{width}}" + ''.join(f"{medians[(case, base_label)] / 1e9:11.3f}" for case in cases))
        for label in labels[1:]:
            cells = []
            for case in cases:
                median = medians.get((case, label))
                base_median = medians[(case, base_label)]
                cells.append(f"{(median - base_median) / base_median * 100:+10.2f}%" if median else f"{'n/a':>11}")
            print(f"{label:{width}}" + ''.join(cells))


if __name__ == '__main__':
    main()
