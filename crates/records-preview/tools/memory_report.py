#!/usr/bin/env python3
"""Reproduce REPORT.md using bundled immutable census inputs and layout JSON.

No compiler build/run or benchmark is involved. Optional --type-populations accepts
{TypeData_kind: count, mapped_cold_materialized: count,
 conditional_cold_materialized: count}; kind totals match the Type memory row.
"""
import argparse
from fractions import Fraction
import json
from pathlib import Path
import re
import sys

PREVIEW = Path(__file__).resolve().parents[1]
ROOT = PREVIEW.parents[1]
SCALE = Fraction(1033, 1000)
LISTS, ITEMS = 3_119_430, 5_407_345
COLD_POPULATIONS = {'mapped_cold_materialized': 'Mapped', 'conditional_cold_materialized': 'Conditional'}


def load(path):
    return json.loads(path.read_text())


def rounded(value):
    return (value.numerator * 2 + value.denominator) // (2 * value.denominator) if isinstance(value, Fraction) else value


def mib(value):
    return f'{float(value) / 1048576:.3f}'


def method_body(source, kind, method):
    """Read the emitted Rust, not the layout/schema estimate of decode work."""
    start = source.index(f'impl {kind}Record {{')
    end = source.find('\nimpl ', start + 1)
    region = source[start:end if end != -1 else len(source)]
    match = re.search(r'pub fn ' + method + r'\(&self\)[^{]+\{', region)
    if match is None:
        raise ValueError(f'no generated {kind}Record::{method}')
    begin = start + match.end()
    cursor, depth = begin, 1
    while depth:
        depth += (source[cursor] == '{') - (source[cursor] == '}')
        cursor += 1
    return source[begin:cursor - 1], source[:start + match.start()].count('\n') + 1


def decode_load_rows(counts):
    source = (PREVIEW / 'src/nodes.rs').read_text()
    rows = []
    for kind, census in sorted(counts['kinds'].items(), key=lambda item: (-item[1]['count'], item[0]))[:10]:
        data, data_line = method_body(source, kind, 'data')
        tail, tail_line = method_body(source, kind, 'tail')
        if 'self.header.' in data:
            raise ValueError(f'{kind}: re-audit data header load accounting')
        reads = re.findall(r'\bself\.tail\.((?:r#)?\w+)', data)
        tail_reads = re.findall(r'\bself\.tail\.((?:r#)?\w+)', tail)
        # Current hot tail methods only check kind (debug) and form a reference.
        if len(re.findall(r'\bself\.header\.kind\b', tail)) != 1 or 'debug_assert_eq!' not in tail or '&self.tail' not in tail:
            raise ValueError(f'{kind}: re-audit tail accessor load accounting')
        rows.append({'kind': kind, 'count': census['count'], 'dataLoads': len(reads),
                     'dataWords': len(set(reads)), 'dataLine': data_line,
                     'tailLoads': len(tail_reads), 'tailDebugHeaderLoads': 1, 'tailLine': tail_line})
    return rows


def calculate(populations=None):
    metrics = load(PREVIEW / 'layout-metrics.json')
    counts = load(PREVIEW / 'inputs/vscode-counts.json')
    records = load(ROOT / 'crates/records.layout.json')
    binders = load(PREVIEW / 'binder-fields.derived.json')
    raw = (PREVIEW / 'inputs/vscode-b3.stderr').read_text()
    b3 = {m[1]: int(m[2]) for m in re.finditer(r'^b3\.([^=]+)=(\d+)$', raw, re.M)}
    memory = {m[1].strip(): int(m[2]) for m in re.finditer(r'^\[memory\] (.*?)\s+(\d+)\s+[\d.]+ MiB$', raw, re.M)}
    by_kind = {n['kind']: n for n in metrics['nodes']}
    assert sum(k['count'] for k in counts['kinds'].values()) == counts['total']
    header = Fraction(counts['total'] * 24) * SCALE
    tails = Fraction(0)
    nodes = []
    for kind, census in counts['kinds'].items():
        count = census['count']
        if kind in by_kind:
            row = by_kind[kind]
            words, tail_bytes = row['words'], row['tailBytes']
        else:
            token = binders['tokens'][kind]
            words = len(token['fields'])
            tail_bytes = ((words + 1) // 2) * 8
        tails += count * tail_bytes * SCALE
        nodes.append({'kind':kind,'count':count,'words':words,'recordBytes':24+tail_bytes,'bytesScaled':count*(24+tail_bytes)*SCALE})
    # A NodeArray header carries pos/end + len/flags (16 B); modifier flags fit
    # the declared syntactic 16-bit domain in the flags word. Each item is u32.
    # Every separately allocated list is rounded to 8 B: odd item counts need 4 B.
    # The supplied census gives no list-length parity histogram, so report bounds.
    list_header, list_items = LISTS * 16, ITEMS * 4
    list_pad_low = (ITEMS % 2) * 4
    list_pad_high = (LISTS - ((LISTS - ITEMS) % 2)) * 4
    type_count = memory['serial types (Type)']
    if populations is not None:
        expected = set(records['types'])
        if (not isinstance(populations, dict) or set(populations) not in (expected, expected | COLD_POPULATIONS.keys())
                or any(type(n) is not int or n < 0 for n in populations.values())):
            raise ValueError('type populations require all 20 TypeData kinds and optionally BOTH cold materialization counts, all nonnegative integers')
        if sum(populations[kind] for kind in expected) != type_count:
            raise ValueError('type populations do not match the Type memory row')
        for key, kind in COLD_POPULATIONS.items():
            if key in populations and populations[key] > populations[kind]:
                raise ValueError(f'{key} exceeds its {kind} population')
    return metrics, counts, records, binders, b3, memory, {
        'header':header, 'tails':tails,'rare':0,'listHeader':list_header,'listItems':list_items,
        'listPaddingMin':list_pad_low,'listPaddingMax':list_pad_high,
        'nodes':nodes,'typeCount':type_count,'populations':populations,
    }


def render(populations=None):
    metrics, counts, records, binders, b3, memory, totals = calculate(populations)
    mapped_cold = ', '.join(records['types']['Mapped']['outlineFields'])
    conditional_cold = ', '.join(records['types']['Conditional']['outlineFields'])
    coverage = 100 * (1 - b3['inference.either_direction.spilled'] / b3['arena.infos.end'])
    call = counts['kinds']['CallExpression']
    call_schema = next(k for k in load(ROOT/'crates/syntax/nodes.schema.json')['nodes'] if k['kindName']=='CallExpression')
    indexes = [i for i,f in enumerate(call_schema['fields']) if f['name'] in ('questionDotToken','typeArguments')]
    joint = sum(n for mask,n in call['masks'].items() if any(mask[i]=='1' for i in indexes))
    lines = ['# Phase 0.3 record layout report', '',
             'Generated by `python3 crates/records-preview/tools/memory_report.py`. '
             'Inputs are bundled under `inputs/`; original local paths and SHA-256 are in `inputs/provenance.json`. '
             'This is layout arithmetic, not a performance result.', '',
             '## Accounting rules and limitations', '',
             '- Node header = 24 B; tail = `8 * ceil(words / 2)` B. '
             'Header word 5 is the dense file-local `index` (parse creation order, starts at 1); typed tails start at byte 24.',
             '- Type header = 24 B including dense per-checker `id`; w5 is the payload tag. '
             'Payload and inline links follow at byte 24, padded to 8 B. '
             'Mapped and Conditional spill only named cold fields when fully inline size exceeds 64 B; hot members remain inline.',
             '- Plain records accept all bit patterns. Nullable `Id<T>` is raw u32, '
             '`OptionalSpan<T>` stores start/len+1, and booleans/enums are integer codecs. '
             'These avoid the invalid-zero `Ref`/`Span` and Rust-enum problems in a literal transcription of SPEC-arena.',
             '- B-3 was measured on an older source revision; population counts are used as a workload model, '
             'not as current binary layout measurements. The fixed Type comparator is the SPEC\'s 40 + 24 = 64 B.',
             '- **The supplied B-3 streams contain no TypeData-kind population rows.** '
             'Without an external population file, per-kind populations and weighted Type totals are unmeasured. '
             'Use `--type-populations path.json` to fill this gap; the script verifies the aggregate against the memory row.',
             '- `data()` builds owned decoded Data values; hot access is `KindRecord::tail()` / `TypeKindRecord::tail()`. '
             'The integrator must price owned decoding through SPEC-0b ceiling (iv). '
             'The safe stub does not demonstrate native address projection or its cost.',
             '', '## AST model on VS Code', '',
             f'Raw census = {counts["total"]:,} nodes; scale exactly 1033/1000 = {float(counts["total"]*SCALE):,.3f}. '
             'Scale counts without rounding per kind, then round each final byte subtotal to nearest integer. '
             'The list counts below already come from tsc-rs and are not scaled.', '',
             '| Component | Bytes | MiB |', '| --- | ---: | ---: |']
    for name, key in [('Headers','header'),('Kind tails including binder words','tails'),('Rare tails','rare'),('List headers (3,119,430 × 16)','listHeader'),('List items (5,407,345 × 4)','listItems')]:
        val=totals[key]
        lines.append(f'| {name} | {rounded(val):,} | {mib(val)} |')
    lo,hi=totals['listPaddingMin'],totals['listPaddingMax']
    lines += [f'| List allocation padding (parity histogram missing) | {lo:,} … {hi:,} | {mib(lo)} … {mib(hi)} |']
    base=sum(totals[k] for k in ('header','tails','rare','listHeader','listItems'))
    lines += [f'| **Total record/list bytes** | **{rounded(base+lo):,} … {rounded(base+hi):,}** | **{mib(base+lo)} … {mib(base+hi)}** |', '',
              'Totals exclude source text, source-span table, escaped-text bytes, parser JSDoc side maps, '
              'allocator metadata/block slack and records not described by this schema. '
              'Odd list lengths add 4 B each; aggregate item count alone cannot determine padding.', '',
              'Rare decision: CallExpression questionDotToken/typeArguments joint occupancy is '
              f'{joint:,} / {call["count"]:,} = **{joint/call["count"]*100:.6f}%**, so it fails the <1% gate. '
              'Its rounded common record would save 8 B, but this does not override the occupancy condition. '
              'No rare tail is extracted.', '']
    lines += ['## Owned data() cost input for ceiling (iv)', '',
              'Top ten kinds by raw VS Code frequency. Counts below are syntactic 32-bit field reads in the emitted '
              '`src/nodes.rs` method bodies, extracted by this script. They are not measured loads or machine instructions. '
              'Each occurrence of `self.tail.FIELD` in data() counts once, including eager scalar-array construction '
              '(even unused binder words) and repeated field reads. Distinct words are shown separately: optimization may '
              'remove unused reads or combine repeats. Data() itself does not read the header; NodeView dispatch, handle decoding, '
              'owned result construction/copying and caller work are additional costs not priced here.', '',
              '`tail()` returns a reference with zero payload reads and no copy. Its kind check adds one header read in debug '
              'and none with debug assertions disabled; subsequent field accesses cost the words the caller selects. '
              'The integrator must price both paths in SPEC-0b ceiling (iv), including optimization and native arena projection.', '',
              '| Kind | VS Code count | data() source reads | Distinct tail words | tail() payload reads | tail() debug header reads | Generated methods (line) |',
              '| --- | ---: | ---: | ---: | ---: | ---: | --- |']
    for row in decode_load_rows(counts):
        lines.append(f'| {row["kind"]} | {row["count"]:,} | {row["dataLoads"]} | {row["dataWords"]} | {row["tailLoads"]} | {row["tailDebugHeaderLoads"]} | src/nodes.rs data:{row["dataLine"]}, tail:{row["tailLine"]} |')
    lines += ['', '## Per-node-kind layouts', '',
              '| Kind | Words | Record B | VS Code count (raw) | Rare | Binder words (Go authority) |',
              '| --- | ---: | ---: | ---: | --- | --- |']
    for node in metrics['nodes']:
        facts=', '.join(f"{f['field']} ({f['goStruct']})" for f in node['binder']['fields']) or '—'
        count=counts['kinds'].get(node['kind'],{}).get('count',0)
        lines.append(f'| {node["kind"]} | {node["words"]} | {node["recordBytes"]} | {count:,} | none | {facts} |')
    lines += ['', 'Token and keyword kinds occurring in this census:', '',
              '| Kind | Words | Record B | VS Code count (raw) | Binder authority |', '| --- | ---: | ---: | ---: | --- |']
    for node in totals['nodes']:
        if node['kind'] not in {n['kind'] for n in metrics['nodes']}:
            token=binders['tokens'][node['kind']]
            facts=', '.join(f"{f['field']} ({f['goStruct']})" for f in token['fields']) or token['goStruct']
            lines.append(f'| {node["kind"]} | {node["words"]} | {node["recordBytes"]} | {node["count"]:,} | {facts} |')
    lines += ['', 'Schema JSDocComment payloads use five words (tag plus four data words). '
              'The four template rawText optionals add a presence word to preserve Some(TextRef(0)). '
              'There is no node jsDoc field. The Go layout alone does not cover every Rust binder write; '
              'see the static writer reconciliation below. Retained Rust maps are excluded from these AST totals.', '',
              '## Rust binder writer reconciliation', '',
              'The derived JSON records all 32 updates of 12 per-node maps across bind/declare/containers/flow/node_util.rs. '
              'It includes entry insertion, removals, mutable table access and identity relocation. Source hashes pin the '
              'manual guard audit; generation fails on drift. A kind guard must constrain the written key. Generic helpers '
              'and non-returning data-match fallbacks are classified as `any`; dispatch/schema evidence is listed separately.', '',
              '| Rust map | Writer sites | Field | Known missing Go tails | Placement |',
              '| --- | ---: | --- | --- | --- |']
    for name, entry in binders['rustWriters']['maps'].items():
        missing = ', '.join(entry['missingTailKinds']) or 'none established; see any writers'
        placement = 'Node.flags header' if entry['field'] == 'flags' else 'Go-backed tails; retain map for other written kinds'
        lines.append(f'| {name} | {len(entry["writers"])} | {entry["field"]} ({entry["status"]}) | {missing} | {placement} |')
    lines += ['', 'SourceFile.end_flow, SwitchStatement.possibly_exhaustive and ConditionalExpression.flow_when_true/false '
              'are explicitly port-specific. This preview retains their Rust maps instead of silently discarding the facts. '
              'For every unbounded helper, the matrix explicitly retains the map on kinds lacking the Go tail word. '
              'Thus the AST table prices the Go-derived tails, not a complete migrated Rust binder. No map occupancy/byte claim is made.', '',
              '## Checker/binder families', '',
              '| Family | repr(C) B | Arena-rounded B | Source |', '| --- | ---: | ---: | --- |']
    for name, record in records['records'].items():
        source=records['sources'][record['source']]
        lines.append(f'| {name} | {record["bytes"]} | {(record["bytes"]+7)//8*8} | `{source["source"]}:{source["line"]}` ({record["source"]}) |')
    lines += ['', 'Symbol is exactly eight words: flags/name/parent/value_declaration/decl0 (5), more list handle (1), tables (1), ordinal (1). '
              'The nullable more handle is zero when absent; the declaration list header owns its length. '
              'In this stub List<T> stores T, so the concrete handle type is Id<List<Id<Node>>>. '
              'Signature has 48 named bytes: its dense per-checker id replaces the former four padding bytes. StructuredMembers is 36 B, IndexInfo 24 B, '
              'InferenceInfo 52 B (56 allocated); the literal 32/20/24 targets cannot hold the requested fields. '
              'InferenceContext is 48 B plus a lazy 16 B rare record; capture vectors retain their independent identities.', '',
              f'B-3 snapshots: {b3["arena.contexts.end"]:,} contexts, {b3["arena.infos.end"]:,} infos, '
              f'{b3["signature.end.count"] if "signature.end.count" in b3 else memory["serial signatures (Signature)"]:,} signatures, '
              f'{memory["serial resolved members"]:,} members, {memory["serial type mappers"]:,} mappers. '
              f'{coverage:.2f}% two-candidate coverage does not eliminate the four inline handles, two overflow descriptors or metadata.', '',
              '## Per-TypeData-kind tails (M5)', '',
              f'Type memory row population = **{totals["typeCount"]:,}**. '
              'The table separates common allocation from the additional per-kind cold links record. '
              'Out-of-line links occupy 12 B (Mapped; 16 B allocated) or 16 B (Conditional) when materialized. '
              'When populated, total bytes can still exceed 64 B; moving the fields does not make that cost disappear.', '',
              '| Kind | Payload words | Links words total | Inline links + pointer words | Tail B | Record B | Full inline B | Extra cold B | VS Code population | Today B/record | Proposed B total |',
              '| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    for t in metrics['types']:
        pop=populations[t['kind']] if populations is not None else None
        lines.append(f'| {t["kind"]} | {t["payloadWords"]} | {t["linksWords"]} | {t["inlineLinksWords"]} | {t["tailBytes"]} | {t["recordBytes"]} | {t["fullInlineBytes"]} | {t["outlineLinksBytes"]} | {pop if pop is not None else "unmeasured"} | 64 | {pop*t["recordBytes"] if pop is not None else "unmeasured"} |')
    if populations is not None:
        common = sum(populations[t['kind']] * t['recordBytes'] for t in metrics['types'])
        lines += ['', f'Weighted common Type bytes: **{common:,}**; today (64 B each): **{totals["typeCount"] * 64:,}**.']
        if all(key in populations for key in COLD_POPULATIONS):
            cold = sum(populations[key] * 16 for key in COLD_POPULATIONS)
            lines += [f'Materialized cold counts: Mapped **{populations["mapped_cold_materialized"]:,}**, Conditional **{populations["conditional_cold_materialized"]:,}**. '
                      f'Additional allocated cold bytes: **{cold:,}**; complete modeled Type record bytes: **{common + cold:,}**.']
    lines += ['', 'The proposed total column counts common records only; add per-kind materialized cold-link counts × extra cold B rounded to 8 B (16 B for both kinds). '
              'Those occupancies are also not present in B-3. Thus even a future kind population file alone '
              'does not establish the complete resident Type byte total. The optional pair of cold counts in tools/README.md enables the complete modeled record total above.', '',
              f'Mapped cold fields: {mapped_cold}. Conditional cold fields: {conditional_cold}. '
              'All other listed links read directly from the typed tail.', '',
              '## Cold maps retained', '']
    for name, family in records['coldMaps'].items():
        lines += [f'### {name}', '', '| Source field | Placement |', '| --- | --- |']
        lines += [f'| {f["name"]} | {f["placement"]} |' for f in family['fields']]
        lines += ['']
    lines += ['No cold map implementation is generated. Signature instantiations stay in the existing '
              'signature_instantiations map. Source field lists/types/line numbers and enum definitions '
              'are checked against live source by generate_records.py --check.', '']
    return '\n'.join(lines)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check',action='store_true')
    parser.add_argument('--type-populations',type=Path)
    args=parser.parse_args()
    text=render(load(args.type_populations) if args.type_populations else None)
    path=PREVIEW/'REPORT.md'
    if args.check:
        if not path.exists() or path.read_text()!=text:
            sys.exit('REPORT.md is stale')
        print('REPORT.md arithmetic reproduced')
    else:
        path.write_text(text)
        print('wrote crates/records-preview/REPORT.md')


if __name__=='__main__':
    main()
