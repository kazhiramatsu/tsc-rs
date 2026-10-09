#!/usr/bin/env python3
"""Vendor the native TypeScript compiler/conformance test inputs at one pinned commit.

usage: vendor_typescript_native.py --commit <full sha> --profile <name> [--check]

Copies, byte for byte and at their upstream paths, into
vendor/typescript-native/<profile>/upstream/:

  tsc/testdata/tests/cases/{compiler,conformance}   every case file
  tsc/testdata/tests/cases/transpile                every transpile case file
  tsc/testdata/tests/lib                            the harness's /.lib files
  tsc/internal/bundled/libs                         the embedded standard libraries
  tsc/internal/diagnostics/diagnosticMessages.json  the diagnostic message catalog
  tsc/internal/diagnostics/loc/<locale>.json.gz     the localized messages (13 locales)
  tsc/internal/tsoptions/{declscompiler,declsbuild,declswatch,commandlineoption,enummaps}.go
  tsc/internal/core/{compileroptions,watchoptions}.go
                                                    the option declarations and enum values
  tsc/testdata/baselines/reference/{compiler,conformance}/*.errors.txt
  tsc/testdata/baselines/reference/{compiler,conformance}/*.js
  tsc/testdata/baselines/reference/{compiler,conformance}/*.js.map
  tsc/testdata/baselines/reference/{compiler,conformance}/*.sourcemap.txt
  tsc/testdata/baselines/reference/{compiler,conformance}/*.types
  tsc/testdata/baselines/reference/{compiler,conformance}/*.symbols
  tsc/testdata/baselines/reference/{compiler,conformance}/*.trace.json
  tsc/testdata/baselines/reference/transpile        every transpile baseline
  tsc/testdata/baselines/reference/tsoptions        every command-line parsing baseline
  tsc/testdata/baselines/reference/config/tsconfigParsing
                                                    every tsconfig parsing baseline
  tsc/testdata/baselines/reference/{tsc,tsbuild}    every tsc and tsc -b baseline
  tsc/testdata/baselines/reference/{tscWatch,tsbuildWatch}
                                                    every tsc --watch and tsc -b --watch baseline
  tsc/testdata/baselines/reference/api              the API encoder's baselines
  tsc/internal/ast/kind_generated.go, tsc/internal/api/encoder/encoder_generated.go
                                                    the kinds and the API encoder's tables

and writes vendor/typescript-native/<profile>/manifest.json with the commit,
each set's Git tree id (a single file's blob id; a filtered baseline set
records only its blob inventory), file and byte counts, and the sorted names of
*all* compiler and conformance reference baselines (a configuration that ran without errors has
other baselines but no .errors.txt; one that emits nothing has no .js; a skipped one has none).

The upstream checkout lives under target/typescript-native/<commit>/ and is a
shallow, blob-filtered clone; only the vendored blobs are fetched. `--check`
verifies an existing vendored tree against its manifest without network access.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
REMOTE = "https://github.com/microsoft/TypeScript.git"
TREES = [
    "tsc/testdata/tests/cases/compiler",
    "tsc/testdata/tests/cases/conformance",
    "tsc/testdata/tests/cases/transpile",
    "tsc/testdata/tests/lib",
    "tsc/internal/bundled/libs",
    "tsc/testdata/baselines/reference/transpile",
    "tsc/testdata/baselines/reference/tsoptions",
    "tsc/testdata/baselines/reference/config/tsconfigParsing",
    "tsc/testdata/baselines/reference/tsc",
    "tsc/testdata/baselines/reference/tsbuild",
    "tsc/testdata/baselines/reference/tscWatch",
    "tsc/testdata/baselines/reference/tsbuildWatch",
    "tsc/testdata/baselines/reference/api",
]
LOCALES = ["cs-CZ", "de-DE", "es-ES", "fr-FR", "it-IT", "ja-JP", "ko-KR", "pl-PL", "pt-BR",
           "ru-RU", "tr-TR", "zh-CN", "zh-TW"]
FILES = [
    "tsc/internal/diagnostics/diagnosticMessages.json",
    # The command-line option declarations the help, --init and
    # --showConfig read (scripts/tsgo_option_declarations.py).
    "tsc/internal/tsoptions/declscompiler.go",
    "tsc/internal/tsoptions/declsbuild.go",
    "tsc/internal/tsoptions/declswatch.go",
    "tsc/internal/tsoptions/commandlineoption.go",
    "tsc/internal/tsoptions/enummaps.go",
    "tsc/internal/core/compileroptions.go",
    "tsc/internal/core/watchoptions.go",
    # The kind numbers and the encoder's per-kind tables the API's source
    # file encoder follows (scripts/generate_api_encoder.py).
    "tsc/internal/ast/kind_generated.go",
    "tsc/internal/api/encoder/encoder_generated.go",
    # The localized diagnostic messages tsgo embeds (gzip-compressed JSON).
    *[f"tsc/internal/diagnostics/loc/{locale}.json.gz" for locale in LOCALES],
]
BASELINE_DIRS = [
    "tsc/testdata/baselines/reference/compiler",
    "tsc/testdata/baselines/reference/conformance",
]
# The baseline kinds the runner compares: errors, the JavaScript/declaration
# emit, the raw source maps, the source-map records and the type and symbol
# baselines. A kind's suffix must not end another kind's suffix (`.js` is
# matched before `.js.map` is ruled out by `baseline_kind`).
BASELINE_SUFFIXES = (".errors.txt", ".js", ".js.map", ".sourcemap.txt", ".types", ".symbols", ".trace.json")


def baseline_kind(name):
    """The vendored baseline suffix of `name`, or None for the other kinds."""
    for suffix in sorted(BASELINE_SUFFIXES, key=len, reverse=True):
        if name.endswith(suffix):
            return suffix
    return None


def git(checkout, *args, binary=False):
    out = subprocess.check_output(["git", "-C", str(checkout), *args])
    return out if binary else out.decode().strip()


def checkout_for(commit):
    path = ROOT / "target/typescript-native" / commit / "upstream"
    if not (path / ".git").exists():
        path.mkdir(parents=True, exist_ok=True)
        subprocess.run(["git", "-C", str(path), "init", "-q"], check=True)
        subprocess.run(["git", "-C", str(path), "remote", "add", "origin", REMOTE], check=True)
        subprocess.run(["git", "-C", str(path), "fetch", "-q", "--depth", "1", "--filter=blob:none",
                        "origin", commit], check=True)
    head = git(path, "rev-parse", "FETCH_HEAD")
    if head != commit:
        raise SystemExit(f"{path}: fetched {head}, expected {commit}")
    return path


def ls_tree(checkout, commit, path):
    """(mode, blob, path) for every blob under `path`, sorted by path."""
    out = git(checkout, "ls-tree", "-r", "--full-tree", commit, "--", path)
    rows = []
    for line in out.splitlines():
        meta, name = line.split("\t", 1)
        mode, kind, blob = meta.split()
        if kind != "blob":
            raise SystemExit(f"{name}: unexpected {kind} entry")
        rows.append((mode, blob, name))
    return sorted(rows, key=lambda row: row[2])


def inventory_digest(rows):
    text = "".join(f"{mode} {blob} {name}\n" for mode, blob, name in rows)
    return hashlib.sha256(text.encode()).hexdigest()


def blob_sha1(data):
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


def fetch_blobs(checkout, commit):
    """Check out only the vendored paths; the partial clone fetches their blobs in one batch."""
    patterns = ([f"/{path}/" for path in TREES] + [f"/{path}" for path in FILES]
                + [f"/{path}/*{suffix}" for path in BASELINE_DIRS for suffix in BASELINE_SUFFIXES])
    subprocess.run(["git", "-C", str(checkout), "sparse-checkout", "init", "--no-cone"], check=True)
    (checkout / ".git/info/sparse-checkout").write_text("\n".join(patterns) + "\n")
    subprocess.run(["git", "-C", str(checkout), "checkout", "-q", "--detach", commit], check=True)


def read_blobs(checkout, blobs):
    """Blob bytes straight from the object store (no working-tree end-of-line conversion)."""
    # The requests go through a file: writing them to a pipe while the
    # output pipe fills would deadlock both processes.
    with tempfile.TemporaryFile() as requests:
        requests.write("".join(f"{blob}\n" for blob in blobs).encode())
        requests.seek(0)
        process = subprocess.Popen(["git", "-C", str(checkout), "cat-file", "--batch"],
                                   stdin=requests, stdout=subprocess.PIPE)
    contents = {}
    for blob in blobs:
        header = process.stdout.readline().decode().split()
        if len(header) != 3 or header[0] != blob or header[1] != "blob":
            raise SystemExit(f"cat-file --batch: unexpected header {header} for {blob}")
        contents[blob] = process.stdout.read(int(header[2]))
        process.stdout.read(1)
    process.wait()
    return contents


def vendor(commit, profile):
    checkout = checkout_for(commit)
    target = ROOT / "vendor/typescript-native" / profile
    upstream = target / "upstream"
    if upstream.exists():
        shutil.rmtree(upstream)
    sets = []
    selected = []
    for path in TREES:
        rows = ls_tree(checkout, commit, path)
        sets.append({"path": path, "git_tree_sha1": git(checkout, "rev-parse", f"{commit}:{path}"),
                     "rows": rows})
        selected.extend(rows)
    for path in FILES:
        rows = ls_tree(checkout, commit, path)
        if len(rows) != 1 or rows[0][2] != path:
            raise SystemExit(f"{path}: expected exactly one blob, found {len(rows)}")
        sets.append({"path": path, "git_blob_sha1": rows[0][1], "rows": rows})
        selected.extend(rows)
    names = []
    for path in BASELINE_DIRS:
        rows = ls_tree(checkout, commit, path)
        names.extend(name for _, _, name in rows)
        for suffix in BASELINE_SUFFIXES:
            kind = [row for row in rows if baseline_kind(row[2]) == suffix]
            sets.append({"path": path, "filter": f"*{suffix}", "rows": kind})
            selected.extend(kind)
    fetch_blobs(checkout, commit)
    contents = read_blobs(checkout, sorted({blob for _, blob, _ in selected}))
    for mode, blob, name in selected:
        data = contents[blob]
        if blob_sha1(data) != blob:
            raise SystemExit(f"{name}: blob content does not hash to {blob}")
        dest = upstream / name
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(data)
        if mode == "100755":
            dest.chmod(0o755)
    manifest = {
        "schema": 1,
        "repository": REMOTE,
        "commit": commit,
        "commit_date": git(checkout, "show", "-s", "--format=%cI", commit),
        "profile": profile,
        "vendored_root": f"vendor/typescript-native/{profile}/upstream",
        "sets": [
            {
                "path": item["path"],
                **({"filter": item["filter"]} if "filter" in item else {}),
                **({"git_tree_sha1": item["git_tree_sha1"]} if "git_tree_sha1" in item else {}),
                **({"git_blob_sha1": item["git_blob_sha1"]} if "git_blob_sha1" in item else {}),
                "blob_inventory_sha256": inventory_digest(item["rows"]),
                "files": len(item["rows"]),
                "bytes": sum((upstream / name).stat().st_size for _, _, name in item["rows"]),
            }
            for item in sets
        ],
        "baseline_names": {
            "path": "baseline-names.txt",
            "entries": len(names),
            "sha256": hashlib.sha256("".join(f"{n}\n" for n in sorted(names)).encode()).hexdigest(),
        },
    }
    (target / "baseline-names.txt").write_text("".join(f"{name}\n" for name in sorted(names)))
    (target / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"vendored {len(selected)} files into {upstream.relative_to(ROOT)}")
    for item in manifest["sets"]:
        print(f"  {item['path']}{' ' + item['filter'] if 'filter' in item else ''}: "
              f"{item['files']} files, {item['bytes']} bytes")
    print(f"  baseline names: {len(names)}")


def check(profile):
    target = ROOT / "vendor/typescript-native" / profile
    manifest = json.loads((target / "manifest.json").read_text())
    upstream = ROOT / manifest["vendored_root"]
    expected_files = set()
    for item in manifest["sets"]:
        base = upstream / item["path"]
        rows = []
        if base.is_file():
            candidates = [base]
        else:
            candidates = [Path(directory) / file
                          for directory, _, files in os.walk(base) for file in files]
        for full in candidates:
            name = str(full.relative_to(upstream))
            if "filter" in item and baseline_kind(name) != item["filter"][1:]:
                continue
            data = full.read_bytes()
            mode = "100755" if os.access(full, os.X_OK) else "100644"
            rows.append((mode, blob_sha1(data), name))
            expected_files.add(name)
        if "git_blob_sha1" in item and [row[1] for row in rows] != [item["git_blob_sha1"]]:
            raise SystemExit(f"{item['path']}: vendored file differs from its recorded blob")
        rows.sort(key=lambda row: row[2])
        if len(rows) != item["files"] or inventory_digest(rows) != item["blob_inventory_sha256"]:
            raise SystemExit(f"{item['path']}: vendored files differ from the manifest")
    present = {str(Path(d, f).relative_to(upstream)) for d, _, fs in os.walk(upstream) for f in fs}
    if present != expected_files:
        raise SystemExit(f"unexpected vendored files: {sorted(present - expected_files)[:5]}")
    names = (target / manifest["baseline_names"]["path"]).read_bytes()
    if hashlib.sha256(names).hexdigest() != manifest["baseline_names"]["sha256"]:
        raise SystemExit("baseline-names.txt differs from the manifest")
    print(f"{profile}: {len(present)} vendored files match the manifest")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--commit")
    parser.add_argument("--profile", required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    if args.check:
        check(args.profile)
        return
    if not args.commit or len(args.commit) != 40:
        raise SystemExit("--commit takes a full 40-character SHA")
    vendor(args.commit, args.profile)


if __name__ == "__main__":
    sys.exit(main())
