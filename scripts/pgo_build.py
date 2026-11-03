#!/usr/bin/env python3
"""Build `tsc-rs` with profile-guided optimization.

pgo_build.py [--jobs N] [--out DIR] PROJECT_DIR[:TSCONFIG] ...

Three steps, each under `target/pgo` so the ordinary `target/release` stays
as it is:

1. An instrumented release build (`-Cprofile-generate`) in
   `target/pgo/instrumented`.
2. Training: the instrumented compiler runs `-p TSCONFIG --noEmit` in every
   PROJECT_DIR (default `tsconfig.json`), one process at a time, writing
   `.profraw` files under `target/pgo/profiles`.
3. `llvm-profdata merge` into `target/pgo/merged.profdata`, then the
   release build with `-Cprofile-use` in `target/pgo/optimized`; the binary
   is copied to `target/pgo/tsc-rs`.

`llvm-profdata` comes from the toolchain's `llvm-tools` component
(`rustup component add llvm-tools`), so its LLVM matches rustc's. The
instrumented build's own build scripts and proc macros write profiles too
(the flags apply to every host-target crate); only the training runs'
`train-*.profraw` files are merged. The training projects' diagnostics are
not checked: any program the compiler accepts trains it. A project the
instrumented compiler cannot read fails the build (its exit code is
reported), so a missing tsconfig is noticed. The compiler leaves through
the C runtime's exit chain when `LLVM_PROFILE_FILE` is set (its ordinary
`_exit` would skip the profile writer).
"""

import argparse
import glob
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def run(command, **kwargs):
    print("+", " ".join(command), flush=True)
    return subprocess.run(command, **kwargs)


def llvm_profdata():
    sysroot = subprocess.run(
        ["rustc", "--print", "sysroot"], capture_output=True, text=True, check=True
    ).stdout.strip()
    candidates = glob.glob(os.path.join(sysroot, "lib", "rustlib", "*", "bin", "llvm-profdata"))
    if not candidates:
        sys.exit("llvm-profdata not found: rustup component add llvm-tools")
    return candidates[0]


def cargo_build(target_dir, rustflags, jobs, *, prefix=(), runner=run, offline=False):
    env = dict(os.environ)
    # Never let a caller's training filename collect build-script profiles.
    env.pop("LLVM_PROFILE_FILE", None)
    env.pop("CARGO_ENCODED_RUSTFLAGS", None)
    env["RUSTFLAGS"] = rustflags
    env["CARGO_TARGET_DIR"] = str(target_dir)
    if sys.platform == "darwin" and ("-Cprofile-generate" in rustflags or "-Cprofile-use" in rustflags):
        # cc-rs forwards Rust PGO flags to clang. Apple clang's profile ABI
        # can differ from rustc's LLVM (mimalloc then crashes in the profile
        # runtime). Native libraries are not part of the Rust checker census.
        for variable in ("CFLAGS", "CXXFLAGS"):
            env[variable] = (env.get(variable, "") + " -fno-profile-generate -fno-profile-use").strip()
    command = ["cargo", "build", "--release", "-j", str(jobs), "-p", "tsc-rs-cli"]
    if offline:
        env["CARGO_NET_OFFLINE"] = "true"
        command += ["--offline", "--locked"]
    result = runner([*prefix, *command], cwd=ROOT, env=env)
    if result.returncode != 0:
        sys.exit(f"cargo build failed ({result.returncode})")
    return os.path.join(target_dir, "release", "tsc-rs")


def instrumented_build(out, jobs, *, entry_counts=False, **kwargs):
    """Shared step 1; build profiles are never training input."""
    profiles = os.path.join(out, "profiles")
    os.makedirs(profiles, exist_ok=True)
    flags = f"-Cprofile-generate={profiles}"
    if entry_counts:
        # IR PGO normally chooses counters on CFG edges. A census needs
        # counter zero to be the entry even when another edge is cheaper.
        flags += " -Cllvm-args=-pgo-instrument-entry=true"
    binary = cargo_build(os.path.join(out, "instrumented"), flags, jobs, **kwargs)
    for stale in glob.glob(os.path.join(profiles, "*.profraw")):
        os.unlink(stale)
    return binary


def profile_environment(profiles, filename="train-%p-%m.profraw"):
    # Setting this also asks the CLI to use the profile-writing C exit chain.
    return dict(os.environ, LLVM_PROFILE_FILE=os.path.join(profiles, filename))


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("projects", nargs="+", metavar="PROJECT_DIR[:TSCONFIG]")
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--out", default=os.path.join(ROOT, "target", "pgo"))
    args = parser.parse_args()

    profdata = llvm_profdata()
    profiles = os.path.join(args.out, "profiles")
    shutil.rmtree(profiles, ignore_errors=True)
    os.makedirs(profiles)

    instrumented = instrumented_build(args.out, args.jobs)

    for spec in args.projects:
        directory, _, tsconfig = spec.partition(":")
        tsconfig = tsconfig or "tsconfig.json"
        if not os.path.isfile(os.path.join(directory, tsconfig)):
            sys.exit(f"no {tsconfig} in {directory}")
        env = profile_environment(profiles)
        result = run(
            [instrumented, "-p", tsconfig, "--noEmit", "--pretty", "false"],
            cwd=directory,
            env=env,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        print(f"  trained on {directory}/{tsconfig} (exit {result.returncode})", flush=True)
        if result.returncode not in (0, 1, 2):
            sys.exit(f"the instrumented compiler failed on {directory}: exit {result.returncode}")

    raw = sorted(glob.glob(os.path.join(profiles, "train-*.profraw")))
    if not raw:
        sys.exit("training produced no .profraw file")
    merged = os.path.join(args.out, "merged.profdata")
    result = run([profdata, "merge", "-o", merged, *raw])
    if result.returncode != 0:
        sys.exit(f"llvm-profdata merge failed ({result.returncode})")

    built = cargo_build(
        os.path.join(args.out, "optimized"),
        f"-Cprofile-use={merged} -Cllvm-args=-pgo-warn-missing-function",
        args.jobs,
    )
    binary = os.path.join(args.out, "tsc-rs")
    shutil.copy2(built, binary)
    print(binary)


if __name__ == "__main__":
    main()
