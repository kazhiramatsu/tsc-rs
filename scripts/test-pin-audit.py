#!/usr/bin/env python3
"""Guard immutable evidence against the ordinary stale-pin repair path."""
import hashlib
import importlib.util
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("pin_audit", Path(__file__).with_name("pin-audit.py"))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class FrozenPins(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.previous = Path.cwd()
        os.chdir(self.temp.name)
        Path("ratchets").mkdir()
        self.artifact = Path("ratchets/reference.json")
        self.artifact.write_bytes(b"original observation\n")
        self.source = Path("consumer.rs")
        digest = hashlib.sha256(self.artifact.read_bytes()).hexdigest()
        self.source.write_text(f'let input = read("ratchets/reference.json",\n"{digest}");\n')

    def tearDown(self):
        os.chdir(self.previous)
        self.temp.cleanup()

    def test_changed_or_missing_artifact_is_rejected_without_repinning(self):
        original = self.source.read_bytes()
        self.assertEqual(audit.audit_frozen_file(str(self.source), 1), [])
        self.artifact.write_bytes(b"changed observation\n")
        with patch.object(audit, "FIX", True):
            self.assertEqual(len(audit.audit_frozen_file(str(self.source), 1)), 1)
            self.assertEqual(self.source.read_bytes(), original)
            self.artifact.unlink()
            self.assertIn("MISSING", audit.audit_frozen_file(str(self.source), 1)[0])
        self.assertEqual(self.source.read_bytes(), original)

    def test_changed_pair_syntax_cannot_silently_skip_a_frozen_pin(self):
        body = self.source.read_text().replace('"ratchets/reference.json",', 'REFERENCE_PATH,')
        self.source.write_text(body)
        with self.assertRaisesRegex(ValueError, "expected 1 frozen path/hash pairs, found 0"):
            audit.audit_frozen_file(str(self.source), 1)

    def test_join_and_local_constant_pins_are_verified(self):
        digest = hashlib.sha256(self.artifact.read_bytes()).hexdigest()
        self.source.write_text(
            f'const REFERENCE: &str = "ratchets/reference.json";\n'
            f'let a = pinned(&workspace.join("ratchets/reference.json"), "{digest}");\n'
            f'let b = [(REFERENCE, "{digest}",)];\n')
        self.assertEqual(audit.audit_frozen_file(str(self.source), 2), [])
        self.artifact.write_bytes(b"changed observation\n")
        self.assertEqual(len(audit.audit_frozen_file(str(self.source), 2)), 2)

    def test_frozen_failure_prevents_current_pin_fixups(self):
        self.artifact.write_bytes(b"changed observation\n")
        with patch.object(audit, "FIX", True), \
             patch.object(audit, "FROZEN", {str(self.source): 1}), \
             patch.object(audit, "discovery", return_value=[]), \
             patch.object(audit, "audit_file") as current:
            self.assertEqual(audit.main(), 1)
            current.assert_not_called()


if __name__ == "__main__":
    unittest.main()
