"""Evidence-boundary checks for the planning join; no native/oracle execution."""
import json
import unittest

import inventory


class EvidenceBoundaries(unittest.TestCase):
    def test_exact_other_profile_does_not_erase_known_divergence(self):
        row = {"hosted_exact": [{"profile": "H2.6c"}], "known_divergences": {"H2.6a": {}}}
        self.assertEqual(inventory.disposition(row), "current-known-divergence")

    def test_original_global_failure_needs_its_own_current_observation(self):
        row = {"global_failure": [{}], "hosted_exact": [{"profile": "H2.5g"}]}
        self.assertEqual(inventory.disposition(row), "historical-global-failure-not-remeasured")
        row["repair_records"] = ["old-repair-receipt"]
        self.assertEqual(inventory.disposition(row), "historical-global-repair-not-remeasured")

    def test_class_repair_needs_fixture_identity_as_well_as_hosted_id(self):
        row = {"class_failure": {"fixture": "fixture.json"}, "hosted_exact": [{"profile": "retained"}]}
        self.assertEqual(inventory.disposition(row), "historical-class-failure-not-remeasured")
        row["same_fixture_hosted_exact"] = "retained"
        self.assertEqual(inventory.disposition(row), "class-repair-confirmed-hosted")

    def test_upstream_and_rewritten_native_controls_never_get_exact_credit(self):
        for row, expected in [({"upstream_exceptions": "retained"}, "upstream-exception-no-native-credit"),
                              ({"direct_divergence": "memoized"}, "current-direct-divergence")]:
            row["hosted_exact"] = [{"profile": "some-other-context"}]
            self.assertEqual(inventory.disposition(row), expected)

    def test_future_band_excludes_only_matching_global_ids(self):
        rows = {"a": {"required_slices": ["H2.5g"]}, "b": {"required_slices": ["H2.5g", "H2.8a"]}}
        self.assertEqual(inventory.future_ids(rows, "H2.5g", {"a", "extra-focused"}, 1), {"b"})
        with self.assertRaises(ValueError):
            inventory.future_ids(rows, "H2.5g", {"a", "extra-focused"}, 0)

    def test_duplicate_case_and_log_id_are_rejected(self):
        with self.assertRaises(ValueError):
            inventory.indexed([{"case_id": "a"}, {"case_id": "a"}])
        with self.assertRaises(ValueError):
            inventory.identifiers(["EXACT a", "EXACT a"], r"EXACT (\S+)")

    def test_saved_snapshot_keeps_all_distinct_populations(self):
        data = json.loads((inventory.HERE / "inventory.v1.json").read_text())
        rows = inventory.indexed(data["cases"])
        self.assertEqual(len(rows), data["summary"]["inventory_unique_ids"])
        self.assertEqual(data["summary"]["global_ids_without_a_disposition"], 0)
        self.assertEqual(sum(bool(r.get("known_divergences")) for r in rows.values()), 20)
        self.assertEqual(sum(len(r.get("known_divergences", {})) for r in rows.values()), 21)
        self.assertEqual(sum(bool(r.get("class_failure")) for r in rows.values()), 128)
        self.assertEqual(sum(bool(r.get("global_failure")) and bool(r.get("repair_records")) for r in rows.values()), 7)
        self.assertEqual(sum(bool(r.get("same_fixture_hosted_exact")) for r in rows.values()), 88)
        self.assertEqual(sum(bool(r.get("parameter_comment_gap")) for r in rows.values()), 5)
        self.assertEqual(sum(bool(r.get("upstream_exceptions")) for r in rows.values()), 4)
        self.assertTrue(all(r.get("triage_owner") for r in rows.values()))


if __name__ == "__main__":
    unittest.main()
