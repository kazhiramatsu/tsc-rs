#!/usr/bin/env python3
"""gate-tax 5-D: mechanical ORDER-topology audit for the chain walk.

Builds the artifact→producer map from each ORDER script's declared
output constants or literal persist call, extracts every `ratchets/*` reference each script
holds (pinned OR pathHash-recorded — any quoted reference is a
consumption edge), and refuses (like ORDER drift) when a referenced
artifact's producer appears LATER in ORDER than its consumer: that
inversion re-mints the producer's cone in round 2 and costs a third full
round every converge (measured on the #475 incident:
owner-graph before comment-scope-witnesses = 4 re-minted rungs).

Imports from an ORDER producer also depend on that producer's outputs: the
H2.8a observer imports inputPath/inventoryPath instead of spelling their paths.
Missing output declarations and duplicate producers refuse. Self-references
are exempt; referenced artifacts with no in-ORDER
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

# These older producers name their outputs differently, or write several.
# This selects declaration names, never suppresses a dependency edge.
OUTPUT_NAMES = {
    "h1-active-transform": ("OUTPUT_RELATIVE_PATH",),
    "h1-emit-oracle": ("PROFILE_RELATIVE_PATH", "OBSERVATION_RELATIVE_PATH"),
    "h2-transition": ("OWNER_RELATIVE_PATH", "CANDIDATE_RELATIVE_PATH", "PROFILE_RELATIVE_PATH"),
    "h2-8a-candidates": ("inputPath", "inventoryPath"),
}
PERSIST_PATTERN = re.compile(r'\bpersist\("(ratchets/[^"\n]+)",\s*\w+,\s*mode\)')
REFERENCE_PATTERN = re.compile(r'"(ratchets/[^"\n]+)"')
IMPORT_PATTERN = re.compile(r'\bfrom\s+["\']\./([^"\'/]+)\.mjs["\']')


def output_artifacts(name, text):
    names = ("TARGET_RELATIVE_PATH", "targetRelativePath", "target", *OUTPUT_NAMES.get(name, ()))
    pattern = re.compile(
        r'\bconst (?:' + "|".join(names) + r')\s*=\s*'
        r'(?:path\.join\((?:WORKSPACE|workspace|root),\s*)?"(ratchets/[^"\n]+)"'
    )
    return set(pattern.findall(text)) | set(PERSIST_PATTERN.findall(text))


def audit(scripts_dir, order, verbose=False):
    scripts = {}
    producer_of = {}
    outputs = {}
    problems = []
    for position, name in enumerate(order):
        script_path = scripts_dir / f"{name}.mjs"
        if not script_path.is_file():
            problems.append(f"{name}: {script_path} does not exist")
            continue
        text = script_path.read_text(encoding="utf-8")
        scripts[name] = (position, text)
        outputs[name] = output_artifacts(name, text)
        if not outputs[name]:
            problems.append(f"TOPOLOGY DRIFT: {name} has no recognized output declaration")
        for artifact in outputs[name]:
            if artifact in producer_of:
                problems.append(f"TOPOLOGY DRIFT: duplicate producer for {artifact}: {producer_of[artifact][0]}, {name}")
            else:
                producer_of[artifact] = (name, position)
    unproduced = set()
    for name, (position, text) in scripts.items():
        own_target = {
            artifact
            for artifact, (producer, _) in producer_of.items()
            if producer == name
        }
        references = set(REFERENCE_PATTERN.findall(text))
        for imported in IMPORT_PATTERN.findall(text):
            references.update(outputs.get(imported, ()))
        for artifact in sorted(references):
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
        # Lower-case targets and path.join targets are real producers too.
        for declaration in ['const target = "ratchets/a.v1.json";',
                            'const target = path.join(workspace, "ratchets/a.v1.json");']:
            (directory / "prod.mjs").write_text(declaration)
            assert len(audit(directory, ["cons", "prod"])) == 1
            assert not audit(directory, ["prod", "cons"])
        # Both outputs and imported paths must participate in ordering.
        (directory / "h2-8a-candidates.mjs").write_text(
            'export const inputPath = "ratchets/input.json";\n'
            'export const inventoryPath = "ratchets/inventory.json";\n'
        )
        (directory / "obs.mjs").write_text(
            'import { inputPath, inventoryPath } from "./h2-8a-candidates.mjs";\n'
            'persist("ratchets/obs.json", artifact, mode);\n'
        )
        inverted = audit(directory, ["obs", "h2-8a-candidates"])
        assert len(inverted) == 2 and all("ORDER INVERSION" in error for error in inverted)
        assert not audit(directory, ["h2-8a-candidates", "obs"])
        for name, variables in OUTPUT_NAMES.items():
            text = "\n".join(f'const {variable} = "ratchets/{index}.json";'
                             for index, variable in enumerate(variables))
            assert len(output_artifacts(name, text)) == len(variables)
        (directory / "duplicate.mjs").write_text('const target = "ratchets/a.v1.json";')
        assert any("duplicate producer" in error for error in audit(directory, ["prod", "duplicate"]))
        (directory / "unknown.mjs").write_text('const unregisteredOutput = "ratchets/unknown.json";')
        assert any("no recognized output" in error for error in audit(directory, ["unknown"]))
        assert any("does not exist" in error for error in audit(directory, ["missing"]))
    print("topology self-test: ok (inversions, imports, multiple outputs, duplicate/missing producers, self-ref)")
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
