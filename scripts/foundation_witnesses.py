"""Bounded standalone foundation contracts, grouped by crate for one build each.

These are API/value/host contracts, including explicit limitations. Test counts
and fixture memberships are not complete compiler compatibility counts.
"""
import json
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SUITES = {
    "syntax-entity-names": {"crate": "syntax", "target": "entity_names", "oracle": "utf16-entity-names"},
    "syntax-meta-property": {"crate": "syntax", "target": "new_meta_property_name", "oracle": "utf16-new-meta-property-name"},
    "syntax-literal-values": {"crate": "syntax", "target": "owned_literal_values", "oracle": "utf16-owned-literal-values",
                              "inputs": ("crates/compiler/tests/fixtures/utf16-literals-adjacent-probes-inputs.json",)},
    "syntax-recovery": {"crate": "syntax", "target": "recovery_provenance", "oracle": "utf16-recovery-boundary"},
    "syntax-scanner-escapes": {"crate": "syntax", "target": "scanner_escape_diagnostics", "oracle": "utf16-scanner-escape-diagnostics"},
    "syntax-template-escapes": {"crate": "syntax", "target": "template_escape_flags"},
    "syntax-template-flags": {"crate": "syntax", "target": "template_flags", "oracle": "utf16-template-flags"},
    "binder-symbol-names": {"crate": "binder", "target": "owned_symbol_names", "oracle": "utf16-binder-names",
                            "fixture": "crates/binder/tests/fixtures/utf16-binder-names.rs"},
    "types-option-numbers": {"crate": "types", "target": "compiler_option_number_contract"},
    "host-memory": {"crate": "host", "target": "compiler_host_contract"},
    "host-filesystem": {"crate": "host", "target": "filesystem_host_contract"},
    "program-bundle-facts": {"crate": "program", "target": "h2_7d_bundle_source_facts", "oracle": "bundle-plan",
                             "fixture": "crates/emitter/tests/fixtures/bundle-plan.json"},
    "program-host-platform": {"crate": "program", "target": "host_platform_smoke_contract"},
    "program-config-paths": {"crate": "program", "target": "utf16_config_paths"},
    "program-module-paths": {"crate": "program", "target": "utf16_module_paths"},
    "program-raw-source": {"crate": "program", "target": "utf16_raw_source_boundary", "oracle": "utf16-raw-source-boundary"},
}


def source(suite):
    spec = SUITES[suite]
    return f"crates/{spec['crate']}/tests/{spec['target']}.rs"


def inputs(suite):
    spec = SUITES[suite]
    paths = {source(suite), *spec.get("inputs", ())}
    if "oracle" in spec:
        paths.add(f"scripts/observe-{spec['oracle']}.mjs")
        paths.add(spec.get("fixture", f"crates/{spec['crate']}/tests/fixtures/{spec['oracle']}.json"))
    return paths


def test_names(suite, platform=None):
    """Read this bounded set's plain #[test] functions; unfamiliar cfg fails closed.

The full target runs without a name filter. Parsing its declared names separately
lets us reject an empty, ignored, filtered or substituted test binary. Windows
names are inventoried, but running Linux does not qualify those branches.
"""
    platform = sys.platform if platform is None else platform
    conditions = {"unix": platform in ("linux", "darwin"), "windows": platform == "win32",
                  'target_os = "linux"': platform == "linux"}
    if platform not in ("linux", "darwin", "win32"):
        raise ValueError(f"unreviewed foundation platform: {platform}")
    names = []
    for attrs, name in re.findall(r'((?:#\[[^\n]+\]\s*)+)fn (\w+)\(', (ROOT / source(suite)).read_text()):
        if "#[test]" not in attrs:
            continue
        attributes = re.findall(r'#\[(.*?)\]', attrs)
        enabled = True
        for attribute in attributes:
            if attribute == "test":
                continue
            if attribute.startswith("cfg(") and attribute.endswith(")"):
                condition = attribute[4:-1]
                if condition in conditions:
                    enabled &= conditions[condition]
                    continue
            raise ValueError(f"{suite}: review test attribute: {attribute}")
        if enabled:
            names.append(name)
    if not names or len(names) != len(set(names)):
        raise ValueError(f"{suite}: empty or duplicate test declarations")
    return sorted(names)


def command(suites):
    if not suites or len(suites) != len(set(suites)) or any(suite not in SUITES for suite in suites):
        raise ValueError("invalid foundation selection")
    crates = {SUITES[suite]["crate"] for suite in suites}
    if len(crates) != 1:
        raise ValueError("foundation Cargo invocation must belong to one crate")
    argv = ["cargo", "test", "--manifest-path", f"crates/{next(iter(crates))}/Cargo.toml"]
    for suite in suites:
        argv.extend(("--test", SUITES[suite]["target"]))
    return [*argv, "--", "--nocapture", "--test-threads=1"]


def verify_output(suites, output):
    # Cargo announces each integration target before the serial libtest output.
    blocks = re.split(r"^\s*Running tests[/\\](\w+)\.rs \([^\n]+\)\s*$", output, flags=re.M)
    expected = {SUITES[suite]["target"]: test_names(suite) for suite in suites}
    actual = {}
    for target, body in zip(blocks[1::2], blocks[2::2]):
        if target in actual or target not in expected:
            raise ValueError(f"unexpected or duplicate foundation target: {target}")
        names = re.findall(r"^test (\w+) \.\.\. ok$", body, re.M)
        summary = re.findall(r"^test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;", body, re.M)
        if sorted(names) != expected[target] or summary != [str(len(names))]:
            raise ValueError(f"{target}: missing, ignored, filtered or substituted foundation tests")
        actual[target] = names
    if set(actual) != set(expected):
        raise ValueError("missing foundation target output")
    return sum(len(names) for names in actual.values())


def run(suites, env):
    if not suites or len(suites) != len(set(suites)) or any(suite not in SUITES for suite in suites):
        raise ValueError("invalid foundation selection")
    env = dict(env, CARGO_TERM_COLOR="never")
    batches = {}
    observers = []
    for suite in suites:
        test_names(suite)  # Refuse unknown attributes/platforms before any build.
        for path in inputs(suite):
            if not (ROOT / path).is_file():
                raise ValueError(f"{suite}: missing registered input: {path}")
        spec = SUITES[suite]
        batches.setdefault(spec["crate"], []).append(suite)
        if "oracle" in spec and spec["oracle"] not in observers:
            observers.append(spec["oracle"])
    started = time.monotonic()
    for observer in observers:
        subprocess.run(["node", f"scripts/observe-{observer}.mjs", "--check"], cwd=ROOT, env=env, check=True)
    oracle_seconds = time.monotonic() - started
    started = time.monotonic()
    passed = 0
    for batch in batches.values():
        result = subprocess.run(command(batch), cwd=ROOT, env=env, text=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, check=False)
        print(result.stdout, end="", flush=True)
        result.check_returncode()
        passed += verify_output(batch, result.stdout)
    print(json.dumps({"foundation_direct": suites, "platform": sys.platform, "targets": len(suites),
                      "tests_passed": passed, "observer_seconds": round(oracle_seconds, 3),
                      "cargo_build_and_replay_seconds": round(time.monotonic()-started, 3)}), flush=True)
