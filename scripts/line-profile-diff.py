#!/usr/bin/env python3
"""Join two line profiles (tsc-rs TSRS_LINE_PROFILE and the tsc line profiler) by (file, line)
and rank the lines where the first does more work than the second.
usage: lineprof-diff.py <rs.tsv> <tsc.tsv> [--top N] [--by self_ms|instantiations|types] [--file substr]"""
import argparse, csv, collections
ap = argparse.ArgumentParser()
ap.add_argument("rs"); ap.add_argument("tsc"); ap.add_argument("--top", type=int, default=25)
ap.add_argument("--by", default="self_ms"); ap.add_argument("--file", default=None)
ap.add_argument("--prefix", default="", help="path prefix to strip from file names in the report")
a = ap.parse_args()
def load(path):
    rows = {}
    for r in csv.DictReader(open(path), delimiter="\t"):
        key = (r["file"], int(r["line"]))
        rows[key] = {k: (float(v) if k not in ("file", "text", "line") else v) for k, v in r.items()}
    return rows
rs, ts = load(a.rs), load(a.tsc)
if a.file:
    rs = {k: v for k, v in rs.items() if a.file in k[0]}; ts = {k: v for k, v in ts.items() if a.file in k[0]}
keys = set(rs) | set(ts)
only_rs, only_ts = [k for k in keys if k not in ts], [k for k in keys if k not in rs]
print(f"rows: rs={len(rs)} tsc={len(ts)} common={len(keys)-len(only_rs)-len(only_ts)} only_rs={len(only_rs)} only_tsc={len(only_ts)}")
for col in ("self_ms", "instantiations", "types"):
    print(f"  total {col:14s} rs={sum(v[col] for v in rs.values()):12.0f}  tsc={sum(v[col] for v in ts.values()):12.0f}")
def val(rows, k, col): return rows[k][col] if k in rows else 0.0
def text(k): return (rs.get(k) or ts.get(k))["text"][:140]
# per-file excess
per_file = collections.Counter()
for k in keys: per_file[k[0]] += val(rs, k, a.by) - val(ts, k, a.by)
print(f"\n== files with the largest {a.by} excess (rs - tsc)")
for f, d in per_file.most_common(12):
    print(f"  {d:12.0f}  {f.replace(a.prefix, '')}")
print(f"\n== lines with the largest {a.by} excess (rs - tsc)")
print(f"{'excess':>10} {'rs':>10} {'tsc':>10} | {'rs_inst':>8} {'tsc_inst':>8} | {'rs_types':>8} {'tsc_types':>9} | {'rs_ms':>8} {'tsc_ms':>7}  location")
for k in sorted(keys, key=lambda k: -(val(rs, k, a.by) - val(ts, k, a.by)))[:a.top]:
    d = val(rs, k, a.by) - val(ts, k, a.by)
    print(f"{d:10.0f} {val(rs,k,a.by):10.0f} {val(ts,k,a.by):10.0f} | {val(rs,k,'instantiations'):8.0f} {val(ts,k,'instantiations'):8.0f} | {val(rs,k,'types'):8.0f} {val(ts,k,'types'):9.0f} | {val(rs,k,'self_ms'):8.1f} {val(ts,k,'self_ms'):7.1f}  {k[0].replace(a.prefix,'')}:{k[1]}")
    print(f"{'':>10} {text(k)}")
