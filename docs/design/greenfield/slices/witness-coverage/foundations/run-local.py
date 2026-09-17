#!/usr/bin/env python3
"""Record the foundation batch's actual commands, logs and immutable inputs."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import time

ROOT = Path(__file__).resolve().parents[6]
HERE = Path(__file__).resolve().parent
TARGETS = {
    "syntax": ["entity_names", "new_meta_property_name", "owned_literal_values", "recovery_provenance",
               "scanner_escape_diagnostics", "template_escape_flags", "template_flags"],
    "binder": ["owned_symbol_names"],
    "types": ["compiler_option_number_contract"],
    "host": ["compiler_host_contract", "filesystem_host_contract"],
    "program": ["h2_7d_bundle_source_facts", "host_platform_smoke_contract", "utf16_config_paths",
                "utf16_module_paths", "utf16_raw_source_boundary"],
}
OBSERVERS = [f"scripts/observe-utf16-{name}.mjs" for name in (
    "entity-names", "new-meta-property-name", "owned-literal-values", "recovery-boundary",
    "scanner-escape-diagnostics", "template-flags", "binder-names", "raw-source-boundary")]
OBSERVERS.append("scripts/observe-bundle-plan.mjs")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("phase", choices=["baseline", "final"])
    parser.add_argument("--label", required=True)
    args = parser.parse_args()
    out = HERE / args.label
    out.mkdir(exist_ok=False)
    paths = subprocess.check_output(["git", "ls-files", "crates/syntax", "crates/binder", "crates/types",
        "crates/host", "crates/program", "crates/diagnostics", "vendor", "Cargo.toml", "Cargo.lock",
        ".cargo/config.toml", ".node-version", "crates/emitter/tests/fixtures/bundle-plan.json",
        "crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json", *OBSERVERS],
        cwd=ROOT, text=True).splitlines()
    if args.phase == "final":
        paths.extend(["scripts/foundation_witnesses.py", "scripts/witness.py", ".github/ci/replay.py"])
    before = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in sorted(set(paths))}
    (out / "source.json").write_text(json.dumps(before, indent=2) + "\n")
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0", CARGO_PROFILE_TEST_DEBUG="0", RUSTC_WRAPPER="",
               CARGO_TERM_COLOR="never")
    commands = [["node", script, "--check"] for script in OBSERVERS]
    if args.phase == "baseline":
        for crate, targets in TARGETS.items():
            commands.append(["cargo", "test", "--manifest-path", f"crates/{crate}/Cargo.toml",
                             *(word for target in targets for word in ("--test", target)),
                             "--", "--nocapture", "--test-threads=1"])
    else:
        commands = [["python3", ".github/ci/replay.py", "witnesses"]]
        import sys
        sys.path.insert(0, str(ROOT / "scripts"))
        import foundation_witnesses
        env["WITNESS_SUITES"] = json.dumps(list(foundation_witnesses.SUITES))
    record = {"phase": args.phase, "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "platform": platform.platform(), "source_sha256": hashlib.sha256((out / "source.json").read_bytes()).hexdigest(),
              "node": subprocess.check_output(["node", "--version"], text=True).strip(),
              "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip(),
              "env": {key: env[key] for key in ("CARGO_BUILD_JOBS", "CARGO_INCREMENTAL", "CARGO_PROFILE_TEST_DEBUG",
                                                "RUSTC_WRAPPER", "CARGO_TERM_COLOR")}, "runs": []}
    if args.phase == "final":
        record["env"]["WITNESS_SUITES"] = env["WITNESS_SUITES"]
    for i, command in enumerate(commands):
        actual = ["taskpolicy", "-b", "nice", "-n", "15", *command] if platform.system() == "Darwin" else command
        print("START", actual, flush=True)
        started = time.monotonic()
        result = subprocess.run(actual, cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        log = out / f"{i}.log.gz"
        log.write_bytes(gzip.compress(result.stdout, mtime=0))
        row = {"argv": actual, "exit": result.returncode, "seconds": round(time.monotonic()-started, 3),
               "log": log.name, "log_sha256": hashlib.sha256(log.read_bytes()).hexdigest(),
               "summaries": [line for line in result.stdout.decode(errors="replace").splitlines()
                             if "test result:" in line or line.startswith("{")]}
        record["runs"].append(row)
        (out / "receipt.json").write_text(json.dumps(record, indent=2) + "\n")
        print(json.dumps(row), flush=True)
    record["inputs_unchanged"] = all(hashlib.sha256((ROOT / name).read_bytes()).hexdigest() == sha for name, sha in before.items())
    record["binaries_sha256"] = {str(file.relative_to(ROOT)): hashlib.sha256(file.read_bytes()).hexdigest()
                                for targets in TARGETS.values() for target in targets
                                for file in (ROOT / "target/debug/deps").glob(target + "-*")
                                if file.is_file() and not file.suffix and os.access(file, os.X_OK)}
    (out / "receipt.json").write_text(json.dumps(record, indent=2) + "\n")
    if not record["inputs_unchanged"]:
        raise SystemExit("inputs changed during measurement")
    return int(any(row["exit"] for row in record["runs"]))


if __name__ == "__main__":
    raise SystemExit(main())
