#!/usr/bin/env python3
"""Absolute conformance comparison against the tsc oracle.

This complements golden-check: instead of classifying only changed diagnostics,
it runs the oracle for every fixture in a tsrs --check-batch snapshot and
reports exact file-level match rates. The scope intentionally matches the
historical gate: primary main.ts/main.tsx diagnostics compared by
(file, code, line, column).

With --strict, diagnostics are compared as complete structured objects:
code/category/source/flags, full span, message chain, and relatedInformation.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from collections import Counter
from concurrent.futures import ProcessPoolExecutor, as_completed

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, ROOT)

import scripts.parallel_classify as classify  # noqa: E402


SPAN_FIELDS = (
    "file",
    "start",
    "length",
    "byteStart",
    "byteLength",
    "startLine",
    "startCol",
    "endLine",
    "endCol",
)


def serializable_diag(d, strict):
    if strict:
        return json.loads(d)
    file, code, loc = d
    return [file, code, list(loc)]


def diag_code(d, strict):
    if strict:
        return json.loads(d).get("code")
    return d[1]


def is_lib_diag(d, strict):
    return diag_code(d, strict) in classify.LIBCODES


def normalize_file(file):
    if file is None:
        return None
    return os.path.basename(file)


def normalize_span(d):
    out = {}
    for field in SPAN_FIELDS:
        value = d.get(field)
        out[field] = normalize_file(value) if field == "file" else value
    return out


def normalize_message(message):
    if message is None:
        return None
    out = {"text": message.get("text")}
    if "code" in message or "category" in message or message.get("next"):
        out["code"] = message.get("code")
        out["category"] = message.get("category")
        out["next"] = [normalize_message(child) for child in (message.get("next") or [])]
    return out


def normalize_related(related):
    out = {
        "code": related.get("code"),
        "category": related.get("category"),
        **normalize_span(related),
        "message": normalize_message(related.get("message")),
    }
    return out


def normalize_diagnostic(diag):
    return {
        "code": diag.get("code"),
        "category": diag.get("category"),
        "source": diag.get("source"),
        "reportsUnnecessary": bool(diag.get("reportsUnnecessary", False)),
        "reportsDeprecated": bool(diag.get("reportsDeprecated", False)),
        **normalize_span(diag),
        "message": normalize_message(diag.get("message")),
        "related": [normalize_related(r) for r in (diag.get("related") or [])],
    }


def strict_diag_key(diag):
    return json.dumps(
        normalize_diagnostic(diag),
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )


def strict_snapshot_diag_set(diags):
    out = set()
    for diag in diags:
        file = diag.get("file")
        if not file or normalize_file(file) not in classify.PRIMARY_FILES:
            continue
        out.add(strict_diag_key(diag))
    return out


def load_snapshot(path, strict):
    if not strict:
        return classify.load(path)

    out = {}
    with open(path, encoding="utf8", errors="replace") as fh:
        for line in fh:
            line = line.rstrip("\n")
            if "\x01" not in line:
                continue
            fixture_path, js = line.split("\x01", 1)
            try:
                data = json.loads(js)
            except json.JSONDecodeError:
                out[fixture_path] = out.get(fixture_path, set())
                continue
            out[fixture_path] = strict_snapshot_diag_set(data.get("diagnostics", []))
    return out


def strict_tsc_diags_one(path, lib):
    work = tempfile.mkdtemp()
    try:
        with open(path, encoding="utf8", errors="replace") as fh:
            fixture = classify.parse_fixture(fh.read())
        if path.endswith(".tsx") and len(fixture["files"]) == 1 and fixture["files"][0][0] == "main.ts":
            fixture["files"][0] = ("main.tsx", fixture["files"][0][1])

        opts = classify.compiler_options_from_directives(fixture["options"])
        if opts is None:
            return None

        roots = [classify.write_fixture_file(work, name, text) for name, text in fixture["files"]]
        roots.extend(classify.extra_root_path(work, name) for name in fixture["extra_root_files"])
        lib_path = os.path.join(work, "lib.tsrs.d.ts")
        shutil.copy(lib, lib_path)
        roots.append(lib_path)
        opts_path = os.path.join(work, "compiler-options.json")
        with open(opts_path, "w", encoding="utf8") as fh:
            json.dump(opts, fh)

        try:
            r = subprocess.run(
                ["node", classify.ORACLE, "--options-json", opts_path, "--all-files", *roots],
                capture_output=True,
                text=True,
                timeout=classify.TSC_TIMEOUT,
            )
            if r.returncode != 0:
                return None
            data = json.loads(r.stdout)
            return strict_snapshot_diag_set(data.get("diagnostics", []))
        except Exception:
            return None
    finally:
        shutil.rmtree(work, ignore_errors=True)


def tsc_worker(args):
    path, lib, strict = args
    if strict:
        return path, strict_tsc_diags_one(path, lib)
    return path, classify.tsc_diags_one(path, lib)


STRICT_MATCH_KEY_FIELDS = ("file", "code", "startLine", "startCol")
STRICT_COMPARE_FIELDS = (
    "category",
    "source",
    "reportsUnnecessary",
    "reportsDeprecated",
    "start",
    "length",
    "byteStart",
    "byteLength",
    "endLine",
    "endCol",
    "message",
    "related",
)


def strict_match_key(diag):
    return tuple(diag.get(field) for field in STRICT_MATCH_KEY_FIELDS)


def strict_field_diffs(tsrs_diag, tsc_diag):
    return [field for field in STRICT_COMPARE_FIELDS if tsrs_diag.get(field) != tsc_diag.get(field)]


def strict_classify_mismatch(fp, fn):
    by_tsc_key = {}
    for diag in fn:
        by_tsc_key.setdefault(strict_match_key(json.loads(diag)), []).append(diag)

    used_tsc = {}
    fielddiff = []
    extra = []
    for tsrs_key in fp:
        tsrs_diag = json.loads(tsrs_key)
        key = strict_match_key(tsrs_diag)
        used = used_tsc.get(key, 0)
        candidates = by_tsc_key.get(key, [])
        if used < len(candidates):
            used_tsc[key] = used + 1
            tsc_diag = json.loads(candidates[used])
            diffs = strict_field_diffs(tsrs_diag, tsc_diag)
            if diffs:
                fielddiff.append({
                    "key": list(key),
                    "code": tsrs_diag.get("code"),
                    "fields": diffs,
                    "tsrs": tsrs_diag,
                    "tsc": tsc_diag,
                })
            else:
                extra.append(tsrs_diag)
        else:
            extra.append(tsrs_diag)

    missing = []
    for key, diags in by_tsc_key.items():
        used = used_tsc.get(key, 0)
        for diag in diags[used:]:
            missing.append(json.loads(diag))
    return fielddiff, extra, missing


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--snapshot", default="/tmp/golden_now.txt")
    ap.add_argument("--lib", default=os.path.join(ROOT, "lib", "lib.tsrs.d.ts"))
    ap.add_argument("--strict", "--strcit", dest="strict", action="store_true")
    ap.add_argument("--out-json", default=None)
    ap.add_argument("--out-txt", default=None)
    ap.add_argument("--progress-every", type=int, default=100)
    args = ap.parse_args()

    if args.out_json is None:
        args.out_json = (
            "/tmp/full_conformance_compare_primary_strict.json"
            if args.strict
            else "/tmp/full_conformance_compare_primary.json"
        )
    if args.out_txt is None:
        args.out_txt = (
            "/tmp/full_conformance_compare_primary_strict.txt"
            if args.strict
            else "/tmp/full_conformance_compare_primary.txt"
        )

    tsrs_by_file = load_snapshot(args.snapshot, args.strict)
    paths = sorted(path for path in tsrs_by_file if os.path.exists(path))
    missing = sorted(set(tsrs_by_file) - set(paths))

    start = time.time()
    print("full_compare_scope: primary main.ts/main.tsx diagnostics")
    print(
        "full_compare_mode: "
        + (
            "strict diagnostic object equality"
            if args.strict
            else "file+code+line+column equality"
        )
    )
    print(f"tsrs_snapshot: {args.snapshot}")
    print(f"fixtures: {len(paths)} (missing_paths={len(missing)})")
    print(f"tsc_workers: {classify.CLASSIFY_JOBS}, tsc_timeout: {classify.TSC_TIMEOUT}s")
    print(f"output_json: {args.out_json}")
    sys.stdout.flush()

    tsc_by_file = {}
    tsc_fail = []
    with ProcessPoolExecutor(max_workers=classify.CLASSIFY_JOBS) as ex:
        futures = {ex.submit(tsc_worker, (path, args.lib, args.strict)): path for path in paths}
        done = 0
        for fut in as_completed(futures):
            done += 1
            try:
                path, tsc = fut.result()
            except Exception:
                path = futures[fut]
                tsc = None
            if tsc is None:
                tsc_fail.append(path)
            else:
                tsc_by_file[path] = tsc
            if done % args.progress_every == 0 or done == len(paths):
                elapsed = time.time() - start
                rate = done / elapsed if elapsed else 0.0
                eta = (len(paths) - done) / rate if rate else 0.0
                print(
                    f"  {done}/{len(paths)} elapsed={elapsed:.1f}s "
                    f"eta={eta:.1f}s tsc_fail={len(tsc_fail)}"
                )
                sys.stdout.flush()

    raw_match = 0
    filtered_match = 0
    raw_fp = []
    raw_fn = []
    filtered_fp = []
    filtered_fn = []
    mismatches = []
    code_fp = Counter()
    code_fn = Counter()
    code_fp_filtered = Counter()
    code_fn_filtered = Counter()
    strict_fielddiff = []
    strict_actual_extra = []
    strict_actual_missing = []
    strict_fielddiff_fields = Counter()
    strict_fielddiff_codes = Counter()
    strict_actual_extra_codes = Counter()
    strict_actual_missing_codes = Counter()

    for path in paths:
        tsc = tsc_by_file.get(path)
        if tsc is None:
            continue
        tsrs = tsrs_by_file.get(path, set())
        fp = sorted(tsrs - tsc)
        fn = sorted(tsc - tsrs)
        if not fp and not fn:
            raw_match += 1

        tsrs_filtered = {d for d in tsrs if not is_lib_diag(d, args.strict)}
        tsc_filtered = {d for d in tsc if not is_lib_diag(d, args.strict)}
        ffp = sorted(tsrs_filtered - tsc_filtered)
        ffn = sorted(tsc_filtered - tsrs_filtered)
        if not ffp and not ffn:
            filtered_match += 1

        strict_raw_fielddiff = []
        strict_raw_actual_extra = []
        strict_raw_actual_missing = []
        if args.strict and (fp or fn):
            strict_raw_fielddiff, strict_raw_actual_extra, strict_raw_actual_missing = strict_classify_mismatch(fp, fn)
            strict_fielddiff.extend((path, d) for d in strict_raw_fielddiff)
            strict_actual_extra.extend((path, d) for d in strict_raw_actual_extra)
            strict_actual_missing.extend((path, d) for d in strict_raw_actual_missing)
            for d in strict_raw_fielddiff:
                strict_fielddiff_codes[d["code"]] += 1
                for field in d["fields"]:
                    strict_fielddiff_fields[field] += 1
            for d in strict_raw_actual_extra:
                strict_actual_extra_codes[d.get("code")] += 1
            for d in strict_raw_actual_missing:
                strict_actual_missing_codes[d.get("code")] += 1

        if fp or fn or ffp or ffn:
            entry = {
                "path": path,
                "raw_fp": [serializable_diag(d, args.strict) for d in fp],
                "raw_fn": [serializable_diag(d, args.strict) for d in fn],
                "gate_filtered_fp": [serializable_diag(d, args.strict) for d in ffp],
                "gate_filtered_fn": [serializable_diag(d, args.strict) for d in ffn],
            }
            if args.strict:
                entry["raw_fielddiff"] = strict_raw_fielddiff
                entry["raw_actual_extra"] = strict_raw_actual_extra
                entry["raw_actual_missing"] = strict_raw_actual_missing
            mismatches.append(entry)

        raw_fp.extend((path, d) for d in fp)
        raw_fn.extend((path, d) for d in fn)
        filtered_fp.extend((path, d) for d in ffp)
        filtered_fn.extend((path, d) for d in ffn)
        for d in fp:
            code_fp[diag_code(d, args.strict)] += 1
        for d in fn:
            code_fn[diag_code(d, args.strict)] += 1
        for d in ffp:
            code_fp_filtered[diag_code(d, args.strict)] += 1
        for d in ffn:
            code_fn_filtered[diag_code(d, args.strict)] += 1

    classified = len(tsc_by_file)
    elapsed = time.time() - start
    comparison_key = (
        "complete diagnostic object: code/category/source/reports flags/full span/message/related"
        if args.strict
        else "file+code+line+column"
    )
    summary = {
        "scope": f"primary main.ts/main.tsx diagnostics; {comparison_key} set equality",
        "strict": args.strict,
        "comparison_key": comparison_key,
        "snapshot": args.snapshot,
        "fixtures_total": len(paths),
        "missing_paths": len(missing),
        "classified": classified,
        "tsc_fail": len(tsc_fail),
        "raw_exact_match_files": raw_match,
        "raw_exact_match_rate": raw_match / classified if classified else None,
        "raw_mismatch_files": classified - raw_match,
        "raw_false_positive_diagnostics": len(raw_fp),
        "raw_false_negative_diagnostics": len(raw_fn),
        "gate_filtered_exact_match_files": filtered_match,
        "gate_filtered_exact_match_rate": filtered_match / classified if classified else None,
        "gate_filtered_mismatch_files": classified - filtered_match,
        "gate_filtered_false_positive_diagnostics": len(filtered_fp),
        "gate_filtered_false_negative_diagnostics": len(filtered_fn),
        "top_raw_fp_codes": code_fp.most_common(20),
        "top_raw_fn_codes": code_fn.most_common(20),
        "top_gate_filtered_fp_codes": code_fp_filtered.most_common(20),
        "top_gate_filtered_fn_codes": code_fn_filtered.most_common(20),
        "tsc_fail_paths": tsc_fail[:200],
        "elapsed_seconds": elapsed,
        "mismatches": mismatches,
    }
    if args.strict:
        summary.update({
            "strict_fielddiff_pairs": len(strict_fielddiff),
            "strict_actual_extra_diagnostics": len(strict_actual_extra),
            "strict_actual_missing_diagnostics": len(strict_actual_missing),
            "strict_fielddiff_fields": strict_fielddiff_fields.most_common(),
            "top_strict_fielddiff_codes": strict_fielddiff_codes.most_common(20),
            "top_strict_actual_extra_codes": strict_actual_extra_codes.most_common(20),
            "top_strict_actual_missing_codes": strict_actual_missing_codes.most_common(20),
        })

    with open(args.out_json, "w", encoding="utf8") as fh:
        json.dump(summary, fh, indent=2)
    with open(args.out_txt, "w", encoding="utf8") as fh:
        slim = {k: v for k, v in summary.items() if k != "mismatches"}
        json.dump(slim, fh, indent=2)
        fh.write("\n")

    print("\nRESULT")
    print(f"classified: {classified}/{len(paths)}; tsc_fail={len(tsc_fail)}")
    if classified:
        print(f"raw_exact: {raw_match}/{classified} = {raw_match / classified * 100:.2f}%")
        print(f"raw_mismatch_files: {classified - raw_match}")
        print(f"raw_FP_diags: {len(raw_fp)}; raw_FN_diags: {len(raw_fn)}")
        print(
            f"gate_filtered_exact: {filtered_match}/{classified} = "
            f"{filtered_match / classified * 100:.2f}%"
        )
        print(f"gate_filtered_mismatch_files: {classified - filtered_match}")
        print(
            f"gate_filtered_FP_diags: {len(filtered_fp)}; "
            f"gate_filtered_FN_diags: {len(filtered_fn)}"
        )
        print(f"top_raw_fp_codes: {code_fp.most_common(10)}")
        print(f"top_raw_fn_codes: {code_fn.most_common(10)}")
    if args.strict:
        print(f"strict_fielddiff_pairs: {len(strict_fielddiff)}")
        print(
            f"strict_actual_extra: {len(strict_actual_extra)}; "
            f"strict_actual_missing: {len(strict_actual_missing)}"
        )
        print(f"strict_fielddiff_fields: {strict_fielddiff_fields.most_common(10)}")
        print(f"top_strict_actual_extra_codes: {strict_actual_extra_codes.most_common(10)}")
        print(f"top_strict_actual_missing_codes: {strict_actual_missing_codes.most_common(10)}")
    print(f"written: {args.out_txt}")
    print(f"written: {args.out_json}")
    return 0 if not tsc_fail else 1


if __name__ == "__main__":
    raise SystemExit(main())
