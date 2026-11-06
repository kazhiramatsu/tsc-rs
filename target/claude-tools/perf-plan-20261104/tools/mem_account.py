#!/usr/bin/env python3
"""Reduce M7 JSONL: python3 -I mem_account.py A.jsonl [B.jsonl] [--json].

Backing IDs are process addresses, valid only inside one begin/end sample.
First reference owns a backing globally, regardless of its owner label. Virtual
reservations, capacity, resident page observations and lifetime peaks are never
added to initialized live bytes. Unknown quantities stay unknown.
"""
import argparse
import json
from pathlib import Path


FIELDS = ('count', 'initialized_bytes', 'capacity_bytes', 'unique_backing_bytes',
          'retired_versions_bytes', 'reserved_virtual_bytes', 'resident_bytes',
          'non_default_entries', 'slots')


def number(row, name, optional=False):
    value = row.get(name)
    if optional and value is None:
        return None
    if type(value) is not int or value < 0:
        raise ValueError(f'{name} must be a nonnegative integer')
    return value


def reduce_sample(begin, records):
    owners = {}
    seen = {}
    duplicate_properties = {}
    metrics = {}
    mappings = {}
    regions = {}
    unknown = set()
    for row in records:
        kind = row['kind']
        if kind == 'mapping':
            region = number(row, 'region')
            if region in mappings:
                raise ValueError('duplicate mapping region')
            mappings[region] = row
            continue
        if kind == 'metric':
            if row['name'] in metrics:
                raise ValueError('duplicate metric: ' + row['name'])
            metrics[row['name']] = number(row, 'bytes', optional=True)
            continue
        if kind == 'unknown':
            unknown.add(row['owner'])
            continue
        if kind == 'duplicate_properties':
            pointer = number(row, 'backing_id')
            size = number(row, 'bytes')
            if pointer in duplicate_properties and duplicate_properties[pointer] != size:
                raise ValueError('inconsistent duplicate properties backing')
            duplicate_properties[pointer] = size
            continue
        if kind not in ('owner', 'backing'):
            raise ValueError('unknown row kind: ' + kind)
        owner = row['owner']
        count = number(row, 'count')
        width = number(row, 'record_width')
        initialized = number(row, 'initialized_bytes', optional=True)
        capacity = number(row, 'capacity_bytes', optional=True)
        if capacity is not None and initialized is not None and initialized > capacity:
            raise ValueError('initialized bytes exceed capacity')
        if kind == 'backing':
            pointer = number(row, 'backing_id')
            shape = (count, width, initialized, capacity)
            if pointer in seen:
                if seen[pointer] != shape:
                    raise ValueError(f'inconsistent shape for backing {pointer}')
                continue
            seen[pointer] = shape
            region = (pointer - 0x400000000000) // (1 << 38)
            if 0 <= region < 43:
                entry = regions.setdefault(region, dict(claimed_bytes=0, known_initialized_bytes=0, initialized_complete=True))
                entry['claimed_bytes'] += capacity or 0
                entry['known_initialized_bytes'] += initialized or 0
                entry['initialized_complete'] &= initialized is not None
        result = owners.setdefault(owner, {name: 0 for name in FIELDS})
        result.setdefault('unique_backing_groups', 0)
        result['unique_backing_groups'] += int(kind == 'backing')
        result.setdefault('record_widths', set()).add(width)
        result.setdefault('known_initialized_bytes', 0)
        result['known_initialized_bytes'] += initialized or 0
        result.setdefault('known_unique_capacity_bytes', 0)
        result['known_unique_capacity_bytes'] += (capacity if kind == 'backing' else row.get('unique_backing_bytes')) or 0
        # Unknown is contagious; do not silently replace it by zero.
        for name in FIELDS:
            value = row.get(name)
            if name == 'unique_backing_bytes' and kind == 'backing':
                value = capacity
            if value is None or result[name] is None:
                result[name] = None
            else:
                result[name] += number({name: value}, name)

    # In the locked release mimalloc build, page_committed and ordinary
    # malloc counters are omitted at MI_STAT=0. Preserve the raw zero, but
    # expose the unavailable measurement as unknown, not zero touched bytes.
    if metrics.get('allocator.live_usable') is None and metrics.get('allocator.page_committed') == 0:
        metrics['allocator.raw_page_committed'] = 0
        metrics['allocator.page_committed'] = None

    for region, mapping in mappings.items():
        regions.setdefault(region, dict(claimed_bytes=0, known_initialized_bytes=0, initialized_complete=True)).update(mapping)

    footprint = metrics.get('os.physical_footprint')
    live = sum(row['known_initialized_bytes'] for row in owners.values())
    initialized_complete = all(row['initialized_bytes'] is not None for row in owners.values())
    capacity_complete = all(row['unique_backing_bytes'] is not None for row in owners.values())
    known_capacity = sum(row['known_unique_capacity_bytes'] for row in owners.values())
    fixed = metrics.get('observer.fixed_working_bytes')
    path = metrics.get('observer.path_capacity_bytes')
    observer = fixed + path if fixed is not None and path is not None else None
    retained = metrics.get('allocator.retained_free')
    retained_upper = metrics.get('allocator.retained_free_upper_bound')
    remainder_before_retained = footprint - live - observer if footprint is not None and observer is not None else None
    remainder = (remainder_before_retained - retained
                 if remainder_before_retained is not None and retained is not None else None)
    remainder_interval = ([remainder_before_retained - retained_upper, remainder_before_retained]
                          if remainder_before_retained is not None and retained_upper is not None else None)
    after_footprint = metrics.get('os.after_observation.physical_footprint')
    before_peak = metrics.get('os.lifetime_peak_footprint')
    after_peak = metrics.get('os.after_observation.lifetime_peak_footprint')
    observer_net_growth = after_footprint - footprint if after_footprint is not None and footprint is not None else None
    observer_peak_growth = after_peak - before_peak if after_peak is not None and before_peak is not None else None
    for row in owners.values():
        row['record_widths'] = sorted(row['record_widths'])
        filled, slots = row['non_default_entries'], row['slots']
        row['fill_ratio'] = filled / slots if filled is not None and slots else None
        row['share_of_footprint'] = row['initialized_bytes'] / footprint if footprint and row['initialized_bytes'] is not None else None
    return dict(instant=begin['instant'], pid=begin.get('pid'), owners=owners, metrics=metrics, regions=regions,
                unique_initialized_live_bytes=live, known_unique_capacity_bytes=known_capacity,
                capacity_complete=capacity_complete, initialized_complete=initialized_complete, observer_bytes=observer,
                observer_net_footprint_change_bytes=observer_net_growth, observer_lifetime_peak_growth_bytes=observer_peak_growth,
                unexplained_remainder_bytes=remainder,
                remainder_before_allocator_retention_bytes=remainder_before_retained,
                unexplained_remainder_interval_bytes=remainder_interval,
                duplicate_properties_unique_lists=len(duplicate_properties),
                duplicate_properties_unique_payload_bytes=sum(duplicate_properties.values()),
                unknown_owners=sorted(unknown), complete=not unknown and initialized_complete and capacity_complete
                and all(row['retired_versions_bytes'] is not None for row in owners.values()) and retained is not None)


def reduce_lines(lines):
    # Stream fragments into the owner/pointer dictionaries. Do not retain the
    # raw rows as well: a large native sidecar can contain millions of them.
    source = iter(enumerate(lines, 1))
    last_line = 0

    def next_row():
        nonlocal last_line
        for line_number, line in source:
            last_line = line_number
            if not line.strip():
                continue
            try:
                row = json.loads(line)
                row['kind']
            except (ValueError, KeyError, TypeError) as error:
                raise ValueError(f'line {line_number}: {error}') from error
            return row
        return None

    samples = []
    while True:
        begin = next_row()
        if begin is None:
            break
        if begin['kind'] != 'begin':
            raise ValueError(f'line {last_line}: row outside sample')
        if begin.get('schema') != 1:
            raise ValueError(f'line {last_line}: unsupported schema')

        def fragments():
            while True:
                row = next_row()
                if row is None:
                    raise ValueError('truncated sidecar (missing end)')
                if row['kind'] == 'begin':
                    raise ValueError('nested begin: concurrent writers are unsupported')
                if row['kind'] == 'end':
                    if row.get('instant') != begin.get('instant'):
                        raise ValueError('unmatched end')
                    if row.get('write_failed'):
                        raise ValueError('observer write failed')
                    return
                yield row
        try:
            samples.append(reduce_sample(begin, fragments()))
        except (ValueError, KeyError, TypeError) as error:
            raise ValueError(f'line {last_line}: {error}') from error
    if not samples:
        raise ValueError('no samples')
    return samples


def load(path):
    with Path(path).open(encoding='utf-8') as source:
        return reduce_lines(source)


def diff(before, after):
    if [s['instant'] for s in before] != [s['instant'] for s in after]:
        raise ValueError('sidecars must have the same ordered instants')
    results = []
    for old, new in zip(before, after):
        owners = {}
        for owner in sorted(old['owners'].keys() | new['owners'].keys()):
            a, b = old['owners'].get(owner), new['owners'].get(owner)
            changes = {}
            for name in FIELDS:
                x = a.get(name) if a else 0
                y = b.get(name) if b else 0
                changes[name] = y - x if x is not None and y is not None else None
            changes['unique_backing_groups'] = (b.get('unique_backing_groups', 0) if b else 0) - (a.get('unique_backing_groups', 0) if a else 0)
            owners[owner] = changes
        metrics_delta = {}
        for name in old['metrics'].keys() | new['metrics'].keys():
            x, y = old['metrics'].get(name), new['metrics'].get(name)
            metrics_delta[name] = y - x if x is not None and y is not None else None
        region_delta = {}
        for region in old['regions'].keys() | new['regions'].keys():
            a, b = old['regions'].get(region, {}), new['regions'].get(region, {})
            changes = {}
            for name in ('claimed_bytes', 'known_initialized_bytes', 'reserved_virtual_bytes',
                         'observed_extent_bytes', 'resident_bytes'):
                x, y = a.get(name), b.get(name)
                changes[name] = y - x if x is not None and y is not None else None
            region_delta[region] = changes
        results.append(dict(instant=new['instant'], owners=owners, metrics_delta=metrics_delta, regions_delta=region_delta,
                            duplicate_properties_payload_delta=new['duplicate_properties_unique_payload_bytes']-old['duplicate_properties_unique_payload_bytes'],
                            initialized_live_delta=new['unique_initialized_live_bytes']-old['unique_initialized_live_bytes'],
                            footprint_delta=(new['metrics']['os.physical_footprint'] - old['metrics']['os.physical_footprint']
                                             if new['metrics'].get('os.physical_footprint') is not None
                                             and old['metrics'].get('os.physical_footprint') is not None else None)))
    return results


def display(value):
    return '?' if value is None else f'{value:,}'


def table(sample):
    print(f"\n{sample['instant']} (bytes; share uses initialized live / current footprint)")
    print('| owner | count | unique backing groups | width(s) | initialized | capacity | unique backing | retired | virtual | resident* | fill | share |')
    print('|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|')
    for name, row in sorted(sample['owners'].items()):
        fill = '?' if row['fill_ratio'] is None else f"{row['fill_ratio']:.2%}"
        share = '?' if row['share_of_footprint'] is None else f"{row['share_of_footprint']:.2%}"
        widths = ','.join(map(str, row['record_widths']))
        values = [display(row[key]) for key in ('count', 'initialized_bytes', 'capacity_bytes',
                                                'unique_backing_bytes', 'retired_versions_bytes',
                                                'reserved_virtual_bytes', 'resident_bytes')]
        print('| ' + ' | '.join([name, values[0], display(row['unique_backing_groups']), widths, *values[1:], fill, share]) + ' |')
    print('Store regions (claims are not extra live bytes; residency covers the observed prefix):')
    print('| region | known initialized | claimed | mapping virtual | prefix resident |')
    print('|---|---:|---:|---:|---:|')
    for region, row in sorted(sample['regions'].items()):
        print('| ' + ' | '.join([str(region), display(row['known_initialized_bytes']) + ('' if row['initialized_complete'] else '+?'),
                                 display(row['claimed_bytes']), display(row.get('reserved_virtual_bytes')), display(row.get('resident_bytes'))]) + ' |')
    print('Process:', ', '.join(f'{k}={display(v)}' for k, v in sorted(sample['metrics'].items())))
    print('Known unique initialized live:', display(sample['unique_initialized_live_bytes']))
    print('Remainder before allocator retention:', display(sample['remainder_before_allocator_retention_bytes']))
    print('Observer net footprint change / lifetime-peak growth:', display(sample['observer_net_footprint_change_bytes']), '/', display(sample['observer_lifetime_peak_growth_bytes']))
    print('Exact unexplained remainder:', display(sample['unexplained_remainder_bytes']))
    print('Remainder interval using retention upper bound:', sample['unexplained_remainder_interval_bytes'])
    print('Properties duplicating member order:', sample['duplicate_properties_unique_lists'], 'unique lists,',
          display(sample['duplicate_properties_unique_payload_bytes']), 'payload bytes')
    print('Incomplete owners:', ', '.join(sample['unknown_owners']) or 'none')
    print('* Whole intersecting pages; resident columns are not additive across allocations.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('sidecar')
    parser.add_argument('other', nargs='?')
    parser.add_argument('--json', action='store_true')
    args = parser.parse_args()
    try:
        result = load(args.sidecar)
        if args.other:
            result = diff(result, load(args.other))
        if args.json or args.other:
            print(json.dumps(result, indent=2))
        else:
            for sample in result:
                table(sample)
    except (OSError, ValueError) as error:
        parser.exit(2, f'mem_account: {error}\n')


if __name__ == '__main__':
    main()
