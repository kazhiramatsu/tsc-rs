#!/usr/bin/env python3
"""Save/check a cumulative lane patch without staging or committing anything."""
import argparse
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[3]
BASE = "aa199222452ffebc99eb4f66cb5b9daeb6cb742f"
TRACKED = [Path("Cargo.toml"), Path("Cargo.lock")]


def additions():
    return [Path("STATUS.md"), *sorted(
        path.relative_to(ROOT) for path in (ROOT / "crates/arena").rglob("*")
        if path.is_file() and "__pycache__" not in path.parts
    )]


def git(*args, cwd=ROOT):
    return subprocess.check_output(["git", *map(str, args)], cwd=cwd)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["save", "verify"])
    parser.add_argument("name", help="NN-topic.patch under target/lane-arena/patches")
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9]{2}-[a-z0-9-]+\.patch", args.name):
        parser.error("expected NN-topic.patch")
    patch = ROOT / "target/lane-arena/patches" / args.name
    paths = additions()
    if args.action == "save":
        contents = git("diff", BASE, "--", *TRACKED)
        for path in paths:
            result = subprocess.run(
                ["git", "diff", "--no-index", "--", "/dev/null", str(path)],
                cwd=ROOT, capture_output=True, check=False,
            )
            if result.returncode not in (0, 1):
                raise RuntimeError(result.stderr.decode())
            contents += result.stdout
        patch.parent.mkdir(parents=True, exist_ok=True)
        patch.write_bytes(contents)
        print(f"Saved {patch.relative_to(ROOT)} ({len(contents)} bytes)")
    else:
        with tempfile.TemporaryDirectory(prefix="lane-arena-patch-") as directory:
            destination = Path(directory)
            for path in TRACKED:
                (destination / path).write_bytes(git("show", f"{BASE}:{path}"))
            git("apply", "--check", patch, cwd=destination)
            git("apply", patch, cwd=destination)
            for path in [*TRACKED, *paths]:
                if (destination / path).read_bytes() != (ROOT / path).read_bytes():
                    raise RuntimeError(f"patch differs from working tree: {path}")
        print(f"{args.name}: applies to base; all {len(TRACKED) + len(paths)} lane files match working tree")


if __name__ == "__main__":
    main()
