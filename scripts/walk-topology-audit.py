#!/usr/bin/env python3
"""gate-tax 5-D: mechanical ORDER-topology audit for the chain walk.

Builds the artifact→producer map from each ORDER script's declared
target artifact const, extracts every `ratchets/*` reference each script
holds (pinned OR pathHash-recorded — any quoted reference is a
consumption edge), and refuses (like ORDER drift) when a referenced
artifact's producer appears LATER in ORDER than its consumer: that
inversion re-mints the producer's cone in round 2 and costs a third full
round every converge (measured on the #475 incident:
owner-graph before comment-scope-witnesses = 4 re-minted rungs).

Self-references are exempt; referenced artifacts with no in-ORDER
producer are frozen/immutable lineage and are allowed (reported under
--verbose only).

Usage: walk-topology-audit.py [--scripts-dir <dir>] <order-name>...
       walk-topology-audit.py --self-test
Exit: 0 clean, 1 inversion(s) reported, 2 usage/self-test failure.
"""
import pathlib
import re
import sys
import tempfile

TARGET_PATTERN = re.compile(
    r'const (?:TARGET_RELATIVE_PATH|targetRelativePath) =\s*\n?\s*"([^"\n]+)"'
)
REFERENCE_PATTERN = re.compile(r'"(ratchets/[^"\n]+)"')


def audit(scripts_dir, order, verbose=False):
    scripts = {}
    producer_of = {}
    problems = []
    for position, name in enumerate(order):
        script_path = scripts_dir / f"{name}.mjs"
        if not script_path.is_file():
            problems.append(f"{name}: {script_path} does not exist")
            continue
        text = script_path.read_text(encoding="utf-8")
        scripts[name] = (position, text)
        target = TARGET_PATTERN.search(text)
        if target and target.group(1).startswith("ratchets/"):
            producer_of[target.group(1)] = (name, position)
    unproduced = set()
    for name, (position, text) in scripts.items():
        own_target = {
            artifact
            for artifact, (producer, _) in producer_of.items()
            if producer == name
        }
        for artifact in sorted(set(REFERENCE_PATTERN.findall(text))):
            if artifact in own_target:
                continue
            producer = producer_of.get(artifact)
            if producer is None:
                unproduced.add(artifact)
                continue
            producer_name, producer_position = producer
            if producer_name != name and producer_position > position:
                problems.append(
                    f"ORDER INVERSION: {name} (position {position}) references "
                    f"{artifact}, produced by {producer_name} at LATER position "
                    f"{producer_position} — move the producer before its consumer"
                )
    if verbose and unproduced:
        print(
            f"topology: {len(unproduced)} referenced artifacts have no in-ORDER "
            "producer (frozen/immutable lineage) — allowed"
        )
    return problems


def self_test():
    with tempfile.TemporaryDirectory() as raw:
        directory = pathlib.Path(raw)
        (directory / "prod.mjs").write_text(
            'const TARGET_RELATIVE_PATH = "ratchets/a.v1.json";\n'
        )
        (directory / "cons.mjs").write_text(
            'const TARGET_RELATIVE_PATH = "ratchets/b.v1.json";\n'
            'const INPUT = "ratchets/a.v1.json";\n'
            'const FROZEN = "ratchets/frozen.v1.json";\n'
        )
        inverted = audit(directory, ["cons", "prod"])
        if len(inverted) != 1 or "ORDER INVERSION" not in inverted[0]:
            print(f"self-test FAILED: inversion not refused: {inverted}")
            return 2
        clean = audit(directory, ["prod", "cons"])
        if clean:
            print(f"self-test FAILED: clean order refused: {clean}")
            return 2
        # a self-reference (own target pinned inside the producer) is exempt
        (directory / "selfpin.mjs").write_text(
            'const TARGET_RELATIVE_PATH = "ratchets/s.v1.json";\n'
            'const OWN = "ratchets/s.v1.json";\n'
        )
        if audit(directory, ["selfpin"]):
            print("self-test FAILED: self-reference was not exempt")
            return 2
    print("topology self-test: ok (inversion refused, clean order passes, self-ref exempt)")
    return 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    scripts_dir = pathlib.Path(__file__).resolve().parent.parent / "crates/oracle"
    verbose = False
    order = []
    rest = list(argv)
    while rest:
        argument = rest.pop(0)
        if argument == "--scripts-dir":
            scripts_dir = pathlib.Path(rest.pop(0))
        elif argument == "--verbose":
            verbose = True
        else:
            order.append(argument)
    if not order:
        print(
            "usage: walk-topology-audit.py [--scripts-dir <dir>] [--verbose] "
            "<order-name>... | --self-test",
            file=sys.stderr,
        )
        return 2
    problems = audit(scripts_dir, order, verbose)
    if problems:
        for problem in problems:
            print(problem)
        return 1
    print(f"topology: ORDER is producer-before-consumer clean ({len(order)} rungs)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
