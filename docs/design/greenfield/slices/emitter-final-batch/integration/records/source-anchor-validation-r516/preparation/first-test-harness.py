#!/usr/bin/env python3
"""Exercise the real L0 generator through the walk's precondition classifier."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import unittest


ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location("walk_static_checks", ROOT / "scripts/walk-static-checks.py")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)
GENERATOR = ROOT / "crates/oracle/l0-option-inventory.mjs"
SOURCE = ROOT / "crates/compiler/src/lib.rs"
ARTIFACT = ROOT / "ratchets/l0-source-options.v1.json"


class L0AnchorPreflight(unittest.TestCase):
    def setUp(self):
        self.original_bytes = {p: p.read_bytes() for p in (SOURCE, ARTIFACT, GENERATOR)}

    def tearDown(self):
        for path, data in self.original_bytes.items():
            self.assertEqual(path.read_bytes(), data, str(path))

    def classify(self, mutation):
        # Mutate only child-process reads. The canonical Rust and artifact
        # remain untouched, and the generator executes its real --check path.
        program = """
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';
const [generator, source, artifact, mutation] = CONFIG;
const needle = '    let mut input = InputFile::from_snapshot(name, Arc::clone(source.snapshot()));';
const read = fs.readFileSync;
fs.readFileSync = function(file, ...args) {
  const value = read.call(this, file, ...args);
  if (String(file) === source && mutation !== 'stale') {
    if (typeof value !== 'string' || value.split(needle).length !== 2) {
      throw new Error('fixture must identify exactly one current snapshot edge');
    }
    return mutation === 'missing' ? value.replace(needle, '') : value + '\n' + needle;
  }
  if (String(file) === artifact && mutation === 'stale') return '{}\n';
  return value;
};
process.argv = ['node', generator, '--check'];
await import(pathToFileURL(generator).href);
""".replace("CONFIG", json.dumps([str(GENERATOR), str(SOURCE), str(ARTIFACT), mutation]))
        label, _, timeout, fragments = next(row for row in CHECKER.CHECKS if row[0] == "l0-option-inventory source anchors")
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = CHECKER.run(label, ["node", "--input-type=module", "-e", program], timeout, fragments)
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


if __name__ == "__main__":
    unittest.main()
