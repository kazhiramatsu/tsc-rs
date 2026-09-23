#!/usr/bin/env python3
"""Summarize a tsc-rs line profile (TSRS_LINE_PROFILE TSV).
usage: scripts/line-profile-report.py <profile.tsv> [--top N] [--by self_ms|instantiations|types|relations|...] [--file substr]"""
import argparse, csv, collections, sys
ap = argparse.ArgumentParser()
ap.add_argument("path"); ap.add_argument("--top", type=int, default=25)
ap.add_argument("--by", default="self_ms"); ap.add_argument("--file", default=None)
a = ap.parse_args()
rows = list(csv.DictReader(open(a.path), delimiter="\t"))
for r in rows:
    for k, v in r.items():
        if k not in ("file", "text", "line"):
            r[k] = float(v)
    r["line"] = int(r["line"])
if a.file:
    rows = [r for r in rows if a.file in r["file"]]
total_self = sum(r["self_ms"] for r in rows)
print(f"rows={len(rows)} total self={total_self:.0f} ms  (inclusive sum {sum(r['inclusive_ms'] for r in rows):.0f} ms)")
per_file = collections.Counter()
for r in rows: per_file[r["file"]] += r["self_ms"]
print("\n== heaviest files (self ms)")
for f, ms in per_file.most_common(12):
    print(f"  {ms:9.1f}  {f}")
key = a.by
print(f"\n== heaviest lines by {key}")
hdr = f"{'self_ms':>9} {'incl_ms':>9} {'hits':>5} {'inst':>8} {'types':>7} {'sig':>6} {'cond':>6} {'rel':>7} {'name':>6} {'mapped':>6} {'idx':>6} {'memb':>6}  location"
print(hdr)
for r in sorted(rows, key=lambda r: -r[key])[:a.top]:
    loc = r["file"] + ":" + str(r["line"])
    print(f"{r['self_ms']:9.1f} {r['inclusive_ms']:9.1f} {int(r['hits']):5d} {int(r['instantiations']):8d} {int(r['types']):7d} {int(r['signatures']):6d} {int(r['conditional']):6d} {int(r['relations']):7d} {int(r['resolve_name']):6d} {int(r['mapped']):6d} {int(r['indexed']):6d} {int(r['members']):6d}  {loc}")
    print(f"{'':>9} {r['text'][:150]}")
