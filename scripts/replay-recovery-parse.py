#!/usr/bin/env python3
"""Build a pinned standalone parser and replay the census's exact parse inputs."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def sha(data):
    return hashlib.sha256(data).hexdigest()


def git(tree, *args):
    return subprocess.check_output(["git", "-C", str(tree), *args])


def source_identity(tree):
    paths = git(tree, "ls-files", "-z", "crates/syntax", "crates/types", "crates/diagnostics", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml").split(b"\0")
    return {os.fsdecode(path): sha((tree / os.fsdecode(path)).read_bytes()) for path in paths if path}


def without_await_projection(parser_bytes):
    start = parser_bytes.index(b"    fn subtree_contains_possible_top_level_await(")
    # Include the helper's documentation, which changed with its implementation.
    start = parser_bytes.rindex(b"\n\n", 0, start) + 2
    end = parser_bytes.index(b"    fn parse_source_element(", start)
    return parser_bytes[:start], parser_bytes[end:]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parser-tree", type=Path, required=True)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--build-dir", type=Path, required=True)
    parser.add_argument("--label", required=True)
    parser.add_argument("--with-recovery-profiles", action="store_true")
    parser.add_argument("--baseline-kind", choices=["candidate", "projection", "merge-base"], required=True)
    args = parser.parse_args()
    assert not args.out.exists(), f"refusing to overwrite replay evidence: {args.out}"
    source_tree = Path(__file__).resolve().parents[1]
    tree = args.parser_tree.resolve()
    build = args.build_dir.resolve()
    build.mkdir(parents=True, exist_ok=True)
    source_hashes = source_identity(tree)
    for relative in ["crates/syntax/src/nodes.rs", "crates/syntax/src/observable_fields.rs", "crates/syntax/src/for_each_child.rs"]:
        assert (tree / relative).read_bytes() == (source_tree / relative).read_bytes(), f"historical digest schema changed: {relative}"
    if args.baseline_kind == "projection":
        assert args.with_recovery_profiles
        parser_path = "crates/syntax/src/parser.rs"
        assert (tree / parser_path).read_bytes() == git(tree, "show", f"81d5aa52e:{parser_path}"), "reverted parser is not the pinned pre-interface projection"
        assert without_await_projection((tree / parser_path).read_bytes()) == without_await_projection((source_tree / parser_path).read_bytes()), "candidate parser changed outside the await projection"
        # Only the known parser projection differs; all recovery predicates,
        # syntax support, types and diagnostics remain the candidate's bytes.
        for relative, digest in source_hashes.items():
            if relative.startswith(("crates/syntax/", "crates/types/", "crates/diagnostics/")) and relative != parser_path:
                assert digest == sha((source_tree / relative).read_bytes()), relative
    elif args.baseline_kind == "merge-base":
        assert not args.with_recovery_profiles
        assert git(tree, "rev-parse", "HEAD").decode().strip() == "3b1f5fe87fd31e3b303bb44bd257342735452ed9"
        assert not git(tree, "diff", "HEAD", "--", "crates/syntax", "crates/types", "crates/diagnostics")
    else:
        assert args.with_recovery_profiles
        assert source_hashes == source_identity(source_tree)
    probe_paths = [source_tree / "scripts/replay-recovery-parse.rs", source_tree / "crates/xtask/src/recovery_parse_snapshot.rs"]
    probe_hashes = {str(path): sha(path.read_bytes()) for path in probe_paths}
    manifest = ['[package]', 'name = "recovery-parse-replay"', 'version = "0.0.0"', 'edition = "2021"', '', '[workspace]', '', '[[bin]]', 'name = "recovery-parse-replay"', f'path = {json.dumps(str(probe_paths[0]))}', '', '[dependencies]', 'base64 = "0.22"', 'serde_json = "1.0"', 'sha2 = "0.10"']
    for name in ["syntax", "types", "diagnostics"]:
        manifest.append(f'tsc-{name} = {{ package = "tsc-rs-{name}", path = {json.dumps(str(tree / "crates" / name))} }}')
    manifest += ['', '[features]', 'current-recovery-profiles = []', '', '[profile.dev]', 'opt-level = 3', 'debug = 0', 'incremental = false', '']
    (build / "Cargo.toml").write_text("\n".join(manifest))
    # Pin dependency resolution to the candidate lock, even for older parsers.
    (build / "Cargo.lock").write_bytes((source_tree / "Cargo.lock").read_bytes())
    env = os.environ.copy()
    env["CARGO_BUILD_JOBS"] = "2"
    env["CARGO_TARGET_DIR"] = str(build / "target")
    prefix = ["taskpolicy", "-b", "nice", "-n", "15"] if sys.platform == "darwin" else []
    started = time.monotonic()
    args.out.parent.mkdir(parents=True, exist_ok=True)
    log = args.out.with_suffix(".build.log")
    with log.open("wb") as output:
        features = ["--features", "current-recovery-profiles"] if args.with_recovery_profiles else []
        subprocess.run(prefix + ["cargo", "build", "--offline", "--manifest-path", str(build / "Cargo.toml")] + features, cwd=source_tree, env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
        subprocess.run(prefix + ["cargo", "test", "--offline", "--manifest-path", str(build / "Cargo.toml")] + features, cwd=source_tree, env=env, stdout=output, stderr=subprocess.STDOUT, check=True)
    binary = build / "target/debug/recovery-parse-replay"
    input_bytes = args.input.read_bytes()
    result = subprocess.run(prefix + [str(binary)], input=input_bytes, stdout=subprocess.PIPE, check=True)
    assert source_hashes == source_identity(tree), "parser source changed during build/replay"
    assert probe_hashes == {str(path): sha(path.read_bytes()) for path in probe_paths}, "probe source changed during build/replay"
    output = json.loads(result.stdout)
    assert output["input_artifact_sha256"] == sha(input_bytes)
    output["build"] = {
        "label": args.label, "baseline_kind": args.baseline_kind,
        "parser_head": git(tree, "rev-parse", "HEAD").decode().strip(),
        "parser_diff_sha256": sha(git(tree, "diff", "HEAD", "--", "crates/syntax", "crates/types", "crates/diagnostics")),
        "source_files_sha256": source_hashes,
        "probe_files_sha256": probe_hashes,
        "binary_sha256": sha(binary.read_bytes()),
        "manifest_sha256": sha((build / "Cargo.toml").read_bytes()),
        "lock_sha256": sha((build / "Cargo.lock").read_bytes()),
        "build_log_sha256": sha(log.read_bytes()),
        "rustc": subprocess.check_output(["rustc", "--version", "--verbose"], cwd=source_tree, text=True),
        "seconds": round(time.monotonic() - started, 3),
    }
    with args.out.open("x") as file:
        json.dump(output, file, separators=(",", ":"))
        file.write("\n")
    print(f'{args.label}: replayed {len(output["digests"])} inputs; {output["build"]["seconds"]}s')


if __name__ == "__main__":
    main()
