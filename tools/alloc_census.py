#!/usr/bin/env python3
"""Reduce a version-1 tsc-rs allocation sidecar (python3 -I; stdlib only).

--symbols accepts an address-to-frame JSON object, {"slide":"0x...",
"symbols":{...}}, or lines '0xADDRESS FRAME' / nm's 'ADDRESS TYPE NAME'.
Use runtime addresses for atos output, or give the binary's ASLR slide for nm.
Sample estimates cover only the post-installation window; preinstall
attempts are separately counted in the header.
realloc_copy_bytes is min(old,new) requested payload for moved reallocs,
not the backend memcpy length. Peaks are gauges: max across
threads/alignments, never sum. Backing virtual
reservation and room claims are reported separately and never added as RSS.
"""
import argparse
import bisect
import collections
import json
import pathlib
import re
import sys

FIELDS = ('alloc', 'alloc_zeroed', 'realloc', 'dealloc', 'failures',
          'requested_bytes', 'realloc_old_bytes', 'realloc_new_bytes', 'freed_bytes',
          'realloc_moved', 'realloc_copy_bytes')
SIZE_BINS = ('<=16', '<=32', '<=64', '<=128', '<=256', '<=1K', '<=4K', '<=64K', '>64K')
LENGTH_BINS = ('0', '1', '2-8', '9-12', '13-20', '21-64', '65-256', '257+')
LIFETIMES = ('never_freed_before_pre_exit', 'same_phase', 'later_phase', 'tracking_lost', 'allocation_failed')


def integer(value, label, minimum=0):
    if type(value) is not int or value < minimum:
        raise ValueError(f'{label}: expected integer >= {minimum}')
    return value


def read_symbols(path):
    if path is None:
        return lambda pc: pc
    text = pathlib.Path(path).read_text()
    slide, symbols, exact = 0, {}, False
    if text.lstrip().startswith('{'):
        obj = json.loads(text)
        if 'symbols' in obj:
            exact = obj.get('exact', False)
            slide = int(str(obj.get('slide', '0')), 0)
            obj = obj['symbols']
        symbols = {int(address, 16): str(frame) for address, frame in obj.items()}
    else:
        for line in text.splitlines():
            if not line.strip() or line.startswith('#'):
                continue
            parts = line.split(maxsplit=1)
            if len(parts) != 2 or not re.fullmatch(r'(?:0x)?[0-9a-fA-F]+', parts[0]):
                raise ValueError('symbols: expected ADDRESS FRAME or nm ADDRESS TYPE NAME')
            frame = re.sub(r'^[a-zA-Z] ', '', parts[1])
            symbols[int(parts[0], 16)] = frame
    addresses = sorted(symbols)

    def symbolize(pc):
        address = int(pc, 16) - slide
        if exact:
            return symbols.get(address, pc)
        pos = bisect.bisect_right(addresses, address) - 1
        if pos < 0:
            return pc
        base = addresses[pos]
        offset = address - base
        return symbols[base] + (f' + 0x{offset:x}' if offset else '')
    return symbolize


def reduce_rows(rows, symbolize=lambda pc: pc):
    rows = iter(rows)
    try:
        header = next(rows)
    except StopIteration as exc:
        raise ValueError('empty sidecar') from exc
    if header.get('type') != 'header' or header.get('format') != 'tsrs-alloc-census':
        raise ValueError('expected tsrs-alloc-census header first')
    if type(header.get('version')) is not int or header['version'] != 1:
        raise ValueError('unsupported sidecar version')
    for flag in ('alloc_census', 'phase_ledger', 'perf_counters', 'config_error'):
        if type(header.get(flag)) is not bool:
            raise ValueError(f'header: missing/invalid {flag}')
    if not header['alloc_census'] or not header['phase_ledger']:
        raise ValueError('required feature flags are disabled')
    rate = integer(header.get('sampling_rate'), 'sampling_rate', 1)
    if rate & (rate - 1):
        raise ValueError('sampling_rate must be a power of two')
    integer(header.get('slot_count'), 'slot_count', 1)
    for field in ('sample_capacity', 'pointer_capacity'):
        integer(header.get(field), field, 0 if header['config_error'] else 1)
    for field in ('overflow_slot', 'sample_attempts', 'dropped_samples', 'table_losses',
                  'table_peak', 'slot_overflow_events', 'preinstall_unsampled_events', 'recursion_events'):
        integer(header.get(field), field)
    if header.get('size_bins') != 9 or header.get('alignment_bins') != 8:
        raise ValueError('unsupported bin schema')
    if header['slot_count'] > header['overflow_slot'] or header['table_peak'] > header['pointer_capacity']:
        raise ValueError('invalid slot/table capacities')
    if header['sample_attempts'] - header['dropped_samples'] > header['sample_capacity']:
        raise ValueError('sample count exceeds capacity')
    blocks = collections.Counter()
    phase_bins = {}
    sites = {}
    backing = collections.Counter(events=0, used_bytes=0)
    logical = collections.Counter(events=0, elements=0, payload_bytes=0)
    stacks = collections.Counter()
    lifetimes = collections.Counter()
    observer, recursive, perf = {}, {}, {}
    seen_blocks, seen_samples, seen_sites = set(), set(), set()
    peak_schema = header.get('peak_schema')
    if peak_schema is not None and (type(peak_schema) is not int or peak_schema != 1):
        raise ValueError('unsupported peak schema')
    if peak_schema == 1:
        integer(header.get('unwind_empty_samples'), 'unwind_empty_samples')
    peak = None
    peak_samples = []
    pre_exit = None
    block_totals = None
    complete = False
    for row in rows:
        if complete:
            raise ValueError('records follow complete marker')
        kind = row.get('type')
        if kind == 'block':
            thread = integer(row.get('thread'), 'thread')
            phase = integer(row.get('phase'), 'phase')
            size = integer(row.get('size_bin'), 'size_bin')
            align = integer(row.get('alignment_bin'), 'alignment_bin')
            if thread > header['overflow_slot'] or phase >= 20 or size >= 9 or align >= 8:
                raise ValueError('block index out of range')
            key = (thread, phase, size, align)
            if key in seen_blocks:
                raise ValueError('duplicate block bucket')
            seen_blocks.add(key)
            values = {field: integer(row.get(field, 0), field) for field in FIELDS}
            blocks.update(values)
            key = (phase, size)
            bucket = phase_bins.setdefault(key, dict(phase=phase, size_bin=SIZE_BINS[size],
                count=0, bytes=0, dealloc=0, failures=0, live_peak=0))
            bucket['count'] += values['alloc'] + values['alloc_zeroed'] + values['realloc']
            bucket['bytes'] += values['requested_bytes']
            bucket['dealloc'] += values['dealloc']
            bucket['failures'] += values['failures']
            bucket['live_peak'] = max(bucket['live_peak'], integer(row.get('live_peak'), 'live_peak'))
        elif kind == 'totals':
            if row.get('level') != 'block' or block_totals is not None:
                raise ValueError('duplicate/unknown totals')
            block_totals = {field: integer(row.get(field, 0), field) for field in FIELDS}
            # Signed event balances are possible for pre-main runtime carry-ins.
            if type(row.get('live_requested_bytes')) is not int:
                raise ValueError('invalid live_requested_bytes')
            block_totals['live_requested_bytes'] = row['live_requested_bytes']
            block_totals['peak_live_requested_bytes'] = integer(row.get('peak_live_requested_bytes'), 'peak_live_requested_bytes')
        elif kind == 'site':
            level = row.get('level')
            if level not in ('logical', 'backing') or not isinstance(row.get('site'), str):
                raise ValueError('invalid site level/name')
            phase = integer(row.get('phase'), 'site.phase')
            length = integer(row.get('length_bin'), 'length_bin')
            if phase >= 20 or length >= 8:
                raise ValueError('site index out of range')
            key = (level, row['site'], phase, length)
            if key in seen_sites:
                raise ValueError('duplicate site bucket')
            seen_sites.add(key)
            count = integer(row.get('count'), 'site.count')
            amount = integer(row.get('reserved_or_length'), 'reserved_or_length')
            used = integer(row.get('used_bytes'), 'used_bytes')
            target = backing if level == 'backing' else logical
            target['events'] += count
            target['reserved_bytes' if level == 'backing' else 'elements'] += amount
            target['used_bytes' if level == 'backing' else 'payload_bytes'] += used
            sites[key] = dict(level=level, site=row['site'], phase=phase,
                length_bin=LENGTH_BINS[length] if level == 'logical' else None,
                count=count, reserved_or_length=amount, used_bytes=used)
        elif kind == 'peak':
            if peak_schema != 1 or peak is not None or row.get('instant') != 'after_check':
                raise ValueError('invalid/duplicate peak checkpoint')
            if type(row.get('serial')) is not bool or type(row.get('live_requested_bytes')) is not int:
                raise ValueError('invalid peak gauges')
            for field in ('snapshots', 'sample_limit', 'live_samples'):
                integer(row.get(field), 'peak.' + field)
            if row['sample_limit'] > min(header['sample_capacity'], header['sample_attempts']):
                raise ValueError('peak limit exceeds collected samples')
            peak = row
        elif kind == 'sample':
            sequence = integer(row.get('sequence'), 'sample.sequence')
            if sequence in seen_samples or sequence >= header['sample_capacity']:
                raise ValueError('invalid/duplicate sample sequence')
            seen_samples.add(sequence)
            lifetime = integer(row.get('lifetime'), 'lifetime')
            if lifetime >= len(LIFETIMES):
                raise ValueError('unknown lifetime')
            pcs = row.get('pcs')
            if not isinstance(pcs, list) or len(pcs) > 16 or any(not isinstance(pc, str) or not re.fullmatch(r'0x[0-9a-fA-F]+', pc) for pc in pcs):
                raise ValueError('PCs must be <=16 hexadecimal strings')
            site = row.get('site', 'unknown')
            if not isinstance(site, str):
                raise ValueError('sample site must be a string')
            stacks[(site, tuple(symbolize(pc) for pc in pcs))] += 1
            lifetimes[LIFETIMES[lifetime]] += 1
            if peak_schema == 1:
                state = integer(row.get('peak_state'), 'peak_state')
                size = integer(row.get('size'), 'sample.size')
                phase = integer(row.get('phase'), 'sample.phase')
                span = integer(row.get('span'), 'sample.span')
                op = integer(row.get('kind'), 'sample.kind')
                success = row.get('success')
                end_kind = integer(row.get('end_kind'), 'sample.end_kind')
                released = row.get('end_released')
                if type(released) is not bool or end_kind > 3 or (state == 2 and (end_kind not in (2, 3) or (end_kind == 3 and not released))):
                    raise ValueError('invalid block release classification')
                if state > 4 or phase >= 20 or op > 2 or type(success) is not bool:
                    raise ValueError('invalid peak sample')
                if (state in (1, 2, 3) and not success) or (state == 2 and lifetime not in (1, 2)) or (state == 3 and lifetime != 3) or (state == 4 and (success or lifetime != 4)):
                    raise ValueError('peak state disagrees with lifetime/success')
                peak_samples.append((sequence, state, size, phase, span, site, tuple(symbolize(pc) for pc in pcs), released))
        elif kind in ('observer', 'recursive'):
            target = observer if kind == 'observer' else recursive
            if target:
                raise ValueError(f'duplicate {kind} record')
            target.update({field: integer(row.get(field, 0), field) for field in FIELDS})
        elif kind == 'perf':
            name = row.get('name')
            if not isinstance(name, str) or name in perf:
                raise ValueError('invalid/duplicate perf counter')
            perf[name] = integer(row.get('value'), name)
        elif kind == 'pre_exit':
            if pre_exit is not None or type(row.get('all_live_requested_bytes')) is not int:
                raise ValueError('invalid/duplicate pre_exit gauges')
            pre_exit = dict(all_live_requested_bytes=row['all_live_requested_bytes'],
                all_peak_live_requested_bytes=integer(row.get('all_peak_live_requested_bytes'), 'all_peak_live_requested_bytes'),
                counters_available=row.get('counters_available', False),
                instructions=integer(row.get('instructions', 0), 'instructions'),
                cycles=integer(row.get('cycles', 0), 'cycles'))
        elif kind == 'complete' and row.get('version') == 1:
            complete = True
        else:
            raise ValueError(f'unknown record type: {kind!r}')
    if not complete or block_totals is None or not observer or not recursive or pre_exit is None:
        raise ValueError('truncated sidecar: missing totals, observer/recursive, or complete marker')
    if any(blocks[field] != block_totals[field] for field in FIELDS):
        raise ValueError('block totals disagree with buckets')
    if seen_samples != set(range(len(seen_samples))) or len(seen_samples) + header['dropped_samples'] != header['sample_attempts']:
        raise ValueError('sample counts disagree with header')
    if lifetimes['tracking_lost'] > header['table_losses']:
        raise ValueError('sample lifetime losses exceed table losses')
    if sum(recursive[field] for field in FIELDS[:4]) != header['recursion_events']:
        raise ValueError('recursive totals disagree with header')
    unbiased = not (header['dropped_samples'] or header['config_error'] or header['recursion_events'] or header['slot_overflow_events'])
    peak_valid = False
    peak_groups = {'live': [], 'freed_before_peak': [], 'reallocated_in_place_before_peak': []}
    peak_classes = {'live': [], 'freed_before_peak': [], 'reallocated_in_place_before_peak': []}
    if peak_schema == 1:
        if peak is None:
            raise ValueError('missing peak checkpoint')
        if sum(state == 1 for _, state, *_ in peak_samples) != peak['live_samples']:
            raise ValueError('peak live samples disagree with table snapshot')
        for sequence, state, *_ in peak_samples:
            if (sequence < peak['sample_limit']) != (state != 0):
                raise ValueError('peak state disagrees with checkpoint window')
        peak_valid = (unbiased and not header['table_losses'] and peak['snapshots'] == 1
                      and peak['serial'] and header.get('unwinder') == 'darwin-arm64-frame-pointer-v1'
                      and header.get('unwind_empty_samples') == 0
                      and all(frames for _, state, _, _, _, _, frames, _ in peak_samples if state in (1, 2)))
        for name, selected in [('live', 1), ('freed_before_peak', 2), ('reallocated_in_place_before_peak', 2)]:
            groups, classes = {}, {}
            for _, state, size, phase, _, site, frames, released in peak_samples:
                if state != selected or (state == 2 and released != (name == 'freed_before_peak')):
                    continue
                for target, key in [(groups, (site, frames)), (classes, size)]:
                    bucket = target.setdefault(key, dict(sampled_count=0, sampled_bytes=0, phases=collections.Counter()))
                    bucket['sampled_count'] += 1
                    bucket['sampled_bytes'] += size
                    bucket['phases'][phase] += size
            def finish(bucket):
                return dict(bucket, phases={str(p): b for p, b in sorted(bucket['phases'].items())},
                            estimated_count=bucket['sampled_count'] * rate if peak_valid else None,
                            estimated_bytes=bucket['sampled_bytes'] * rate if peak_valid else None)
            peak_groups[name] = [dict(finish(bucket), explicit_site=site, frames=list(frames))
                                 for (site, frames), bucket in sorted(groups.items(), key=lambda pair: (-pair[1]['sampled_bytes'], pair[0]))]
            peak_classes[name] = [dict(finish(bucket), requested_size=size)
                                  for size, bucket in sorted(classes.items(), key=lambda pair: (-pair[1]['sampled_bytes'], pair[0]))]
    top = [dict(sampled_count=count, explicit_site=site,
                estimated_events=count * rate if unbiased and (stack or site != 'unknown') else None,
                frames=list(stack), stack_available=bool(stack))
           for (site, stack), count in stacks.most_common()]
    # Mapping envelope and range claims are intentionally separate. Do not
    # expose their sum under a physical-memory-sounding field name.
    backing.pop('reserved_bytes', None)
    backing['reserved_bytes_note'] = 'see site rows: virtual envelope overlaps ranges; never sum as footprint'
    block_totals['all_categories'] = {field: blocks[field] + observer[field] + recursive[field] for field in FIELDS}
    block_totals['all_categories_live'] = pre_exit
    return dict(header=header, totals=dict(block=block_totals, backing=dict(backing), logical=dict(logical)),
                phase_size_bins=[phase_bins[key] for key in sorted(phase_bins)],
                exact_sites=[sites[key] for key in sorted(sites)], top_sites=top,
                lifetimes=dict(lifetimes), observer=observer, recursive=recursive, perf=perf,
                sampling_estimates_valid=unbiased,
                losses=dict({key: header[key] for key in ('dropped_samples', 'table_losses', 'table_peak',
                    'slot_overflow_events', 'preinstall_unsampled_events', 'recursion_events', 'config_error')},
                    sample_buffer_saturated=header['dropped_samples'] > 0,
                    sample_buffer_full=header['sample_capacity'] > 0 and len(seen_samples) == header['sample_capacity'],
                    pointer_table_full=header['pointer_capacity'] > 0 and header['table_peak'] == header['pointer_capacity']),
                peak=peak, peak_estimates_valid=peak_valid, peak_by_stack=peak_groups,
                peak_by_requested_size=peak_classes,
                peak_note='Requested Rust GlobalAlloc bytes; freed volume is churn, not retained slots or page residency. Moved realloc/dealloc release blocks; in-place realloc closes a lifetime and is reported separately. Sampling window starts after install.',
                stack_qualification=header.get('unwinder', 'unknown'))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sidecar', type=pathlib.Path)
    parser.add_argument('--symbols', type=pathlib.Path)
    parser.add_argument('--json', action='store_true', help='machine-readable output (also the default)')
    args = parser.parse_args(argv)
    try:
        with args.sidecar.open() as stream:
            result = reduce_rows((json.loads(line) for line in stream), read_symbols(args.symbols))
        print(json.dumps(result, indent=2, sort_keys=True))
    except (ValueError, OSError, KeyError, TypeError) as exc:
        parser.exit(2, f'alloc_census: {exc}\n')


if __name__ == '__main__':
    main()
