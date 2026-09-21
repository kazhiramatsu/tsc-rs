#!/usr/bin/env python3
"""Exercise real source-anchor failures through the walk's precondition classifier."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("walk_static_checks", ROOT / "scripts/walk-static-checks.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)
GENERATOR = ROOT / "crates/oracle/l0-option-inventory.mjs"
SOURCE = ROOT / "crates/compiler/src/lib.rs"
ARTIFACT = ROOT / "ratchets/l0-source-options.v1.json"


class SourceAnchorPreflight(unittest.TestCase):
    def setUp(self):
        paths = (
            SOURCE, ARTIFACT, GENERATOR,
            ROOT / "crates/oracle/h2-7a-owner-inventory.mjs",
            ROOT / "crates/oracle/h2-7a-close.mjs",
            ROOT / "crates/emitter/src/declarations/orchestration.rs",
            ROOT / "crates/emitter/src/plan.rs",
        )
        self.original_bytes = {p: p.read_bytes() for p in paths}

    def tearDown(self):
        for path, data in self.original_bytes.items():
            self.assertEqual(path.read_bytes(), data, str(path))

    def classify(self, mutation, generator=GENERATOR, source=SOURCE,
                 needle="    let mut input = InputFile::from_snapshot(name, Arc::clone(source.snapshot()));",
                 check_label="l0-option-inventory source anchors"):
        # Mutate only child-process reads. The canonical Rust and artifact
        # remain untouched, and the generator executes its real --check path.
        program = r"""
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
const [generator, source, artifact, mutation, needle] = CONFIG;
const read = fs.readFileSync;
fs.readFileSync = function(file, ...args) {
  const value = read.call(this, file, ...args);
  if (String(file) === source && mutation !== 'stale') {
    const text = Buffer.isBuffer(value) ? value.toString('utf8') : value;
    if (typeof text !== 'string' || text.split(needle).length !== 2) {
      throw new Error('fixture must identify exactly one current snapshot edge');
    }
    const changed = mutation === 'missing' ? text.replace(needle, '') : text.replace(needle, needle + '\n' + needle);
    return Buffer.isBuffer(value) ? Buffer.from(changed, 'utf8') : changed;
  }
  if (String(file) === artifact && mutation === 'stale') return '{}\n';
  return value;
};
process.argv = ['node', generator, '--check'];
await import(pathToFileURL(generator).href);
""".replace("CONFIG", json.dumps([str(generator), str(source), str(ARTIFACT), mutation, needle]))
        label, _, timeout, fragments = next(row for row in CHECKER.CHECKS if row[0] == check_label)
        command = ["node", "--input-type=module", "-e", program]
        if mutation == "stale":
            completed = subprocess.run(command, capture_output=True, text=True, timeout=timeout, check=False)
            self.assertEqual(completed.returncode, 1)
            self.assertIn("stale ratchets/l0-source-options.v1.json", completed.stderr)
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = CHECKER.run(label, command, timeout, fragments)
        return result, output.getvalue()

    def test_missing_and_duplicate_source_anchors_refuse(self):
        for mutation, message in (("missing", "missing audited source fragment"), ("duplicate", "audited source fragment is not unique")):
            with self.subTest(mutation=mutation):
                result, output = self.classify(mutation)
                self.assertEqual(result, 2, output)
                self.assertIn(message, output)

    def test_stale_artifact_is_left_to_the_official_walk(self):
        result, output = self.classify("stale")
        self.assertEqual(result, 0, output)
        self.assertIn("exit 1 (stale or other: the walk's job)", output)

    def test_h2_owner_header_failure_refuses(self):
        result, output = self.classify(
            "missing", ROOT / "crates/oracle/h2-7a-owner-inventory.mjs",
            ROOT / "crates/emitter/src/declarations/orchestration.rs",
            "fn collect_linked_aliases_for_declaration(",
            "h2-7a-owner-inventory curated anchors",
        )
        self.assertEqual(result, 2, output)
        self.assertIn("H2.7b Rust anchor collectLinkedAliases@116719 header changed", output)

    def test_h2_retained_header_and_duplicate_marker_refuse(self):
        for mutation, needle, message in (
            ("missing", "    pub fn validate_bootstrap_shape(&self) -> Result<(), EmitFailure> {", "does not start validate_bootstrap_shape"),
            ("duplicate", "                    EmitContractViolation::ScriptOutputMissingJavaScriptPath,", "must have one live occurrence inside validate_bootstrap_shape"),
        ):
            with self.subTest(mutation=mutation):
                result, output = self.classify(
                    mutation, ROOT / "crates/oracle/h2-7a-close.mjs",
                    ROOT / "crates/emitter/src/plan.rs", needle,
                    "h2-7a-close retained arms / anchors",
                )
                self.assertEqual(result, 2, output)
                self.assertIn(message, output)


if __name__ == "__main__":
    unittest.main()
