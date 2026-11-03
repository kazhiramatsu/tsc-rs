"""Checks for cross-port count normalization and comparison edge cases."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch


SPEC = importlib.util.spec_from_file_location(
    "tsgo_census", Path(__file__).resolve().parents[1] / "tsgo_census.py"
)
census = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(census)


class CensusTests(unittest.TestCase):
    def test_go_and_rust_names_share_keys(self):
        self.assertEqual(census.normalize({
            "CompareTypes": 3,
            "internal/checker/checker.go:Checker.getPropertyOfType": 4,
            "tsc_checker::State::get_effects_signature::h0123456789abcdef": 5,
            "tsc_checker::State::get_property_of_type_full::<Name>": 6,
        }), {"compareTypes": 3, "getPropertyOfType": 10, "getEffectsSignature": 5})

    def test_monomorphizations_sum_but_closures_do_not(self):
        closure = "tsc_checker::State::infer_types::{{closure}}"
        self.assertEqual(census.normalize({
            "tsc_checker::State::infer_types::<First>": 7,
            "tsc_checker::State::infer_types::<Second>": 11,
            closure: 100,
        }), {"inferTypes": 18, closure: 100})

    def test_wrapper_and_body_mapping_stay_separate(self):
        self.assertEqual(census.normalize({
            "get_property_of_type_full": 2,
            "get_property_of_type_ex_with_include_type_only_members": 9,
            "check_expression": 12,
            "check_expression_with_force_tuple": 15,
        }), {"getPropertyOfType": 2, "getPropertyOfTypeEx": 9,
             "checkExpression": 12, "checkExpressionEx": 15})

    def test_explicit_regex_and_ambiguity(self):
        with patch.object(census, "manifest_rows", return_value=[
            ("one", "a.go:one", r"re:^mangled_one_[0-9]+$", ""),
        ]):
            self.assertEqual(census.normalize({"mangled_one_123": 4}), {"one": 4})
        with patch.object(census, "manifest_rows", return_value=[
            ("one", "a.go:one", "re:shared", ""),
            ("two", "b.go:two", "re:shared", ""),
        ]):
            with self.assertRaisesRegex(ValueError, "ambiguous"):
                census.normalize({"shared": 4})

    def test_ratio_direction_order_zero_and_absence(self):
        with tempfile.TemporaryDirectory() as directory:
            a, b = Path(directory) / "a.json", Path(directory) / "b.json"
            a.write_text(json.dumps({"same": 5, "up": 10, "down": 10,
                                     "zero": 0, "infinite": 0, "absent": 7}))
            b.write_text(json.dumps({"same": 5, "up": 30, "down": 5,
                                     "zero": 0, "infinite": 1}))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(census.compare(SimpleNamespace(a=a, b=b)), 0)
            rows = [line.split() for line in output.getvalue().splitlines()[1:]]
            self.assertEqual([r[0] for r in rows],
                             ["absent", "infinite", "up", "down", "same", "zero"])
            self.assertEqual(rows[0], ["absent", "7", "-", "missing"])
            self.assertEqual(rows[1][-1], "inf")
            self.assertEqual(rows[2][-1], "3.000000")
            self.assertEqual(rows[-1][-1], "1.000000")

    def test_invalid_entry_counts_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "counts.json"
            for value in ([1], {"f": True}, {"f": -1}, {"f": 1.5}, {"f": "3"}):
                with self.subTest(value=value):
                    path.write_text(json.dumps(value))
                    with self.assertRaises(ValueError):
                        census.read_counts(path)


if __name__ == "__main__":
    unittest.main()
