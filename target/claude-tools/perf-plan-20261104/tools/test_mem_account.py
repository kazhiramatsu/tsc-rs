#!/usr/bin/env python3
"""python3 -I test_mem_account.py (stdlib only)."""
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('mem_account', Path(__file__).with_name('mem_account.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def owner(name='a', **kwargs):
    row = dict(kind='owner', owner=name, count=2, record_width=4,
               initialized_bytes=8, capacity_bytes=16, unique_backing_bytes=16,
               retired_versions_bytes=None, reserved_virtual_bytes=9999999,
               resident_bytes=4096, non_default_entries=1, slots=2)
    row.update(kwargs)
    return row


def sample(rows, instant='after_check'):
    metrics = [dict(kind='metric', name=k, bytes=v) for k, v in {
        'os.physical_footprint': 100, 'os.lifetime_peak_footprint': 9999,
        'observer.fixed_working_bytes': 3, 'observer.path_capacity_bytes': 2,
        'allocator.retained_free': None, 'allocator.retained_free_upper_bound': 20}.items()]
    return m.reduce_sample(dict(instant=instant, pid=1), metrics + rows)


class Arithmetic(unittest.TestCase):
    def test_initialized_is_separate_from_capacity_virtual_residence_peak(self):
        result = sample([owner()])
        self.assertEqual(result['unique_initialized_live_bytes'], 8)
        self.assertEqual(result['known_unique_capacity_bytes'], 16)
        self.assertEqual(result['remainder_before_allocator_retention_bytes'], 87)
        self.assertEqual(result['unexplained_remainder_interval_bytes'], [67, 87])
        self.assertIsNone(result['unexplained_remainder_bytes'])
        self.assertAlmostEqual(result['owners']['a']['share_of_footprint'], .08)

    def test_unknown_initialization_is_not_charged_as_capacity(self):
        result = sample([owner(initialized_bytes=None)])
        self.assertEqual(result['unique_initialized_live_bytes'], 0)
        self.assertFalse(result['initialized_complete'])
        self.assertIsNone(result['owners']['a']['share_of_footprint'])

    def test_region_claims_are_not_added_twice(self):
        rows = [owner(kind='backing', backing_id=0x400000000000),
                dict(kind='mapping', region=0, reserved_virtual_bytes=1 << 38,
                     resident_bytes=4096, observed_extent_bytes=16)]
        result = sample(rows)
        self.assertEqual(result['unique_initialized_live_bytes'], 8)
        self.assertEqual(result['regions'][0]['claimed_bytes'], 16)
        self.assertEqual(result['regions'][0]['resident_bytes'], 4096)

    def test_observer_os_changes_remain_signed(self):
        rows = [dict(kind='metric', name=k, bytes=v) for k, v in {
            'os.physical_footprint': 100, 'os.lifetime_peak_footprint': 200,
            'os.after_observation.physical_footprint': 90,
            'os.after_observation.lifetime_peak_footprint': 210}.items()]
        result = m.reduce_sample(dict(instant='after_check'), rows)
        self.assertEqual(result['observer_net_footprint_change_bytes'], -10)
        self.assertEqual(result['observer_lifetime_peak_growth_bytes'], 10)

    def test_disabled_page_statistics_stay_unknown(self):
        result = m.reduce_sample(dict(instant='after_check'), [
            dict(kind='metric', name='allocator.live_usable', bytes=None),
            dict(kind='metric', name='allocator.page_committed', bytes=0)])
        self.assertIsNone(result['metrics']['allocator.page_committed'])
        self.assertEqual(result['metrics']['allocator.raw_page_committed'], 0)

    def test_exact_retention(self):
        result = m.reduce_sample(dict(instant='after_check'), [owner(),
            dict(kind='metric', name='os.physical_footprint', bytes=100),
            dict(kind='metric', name='observer.fixed_working_bytes', bytes=3),
            dict(kind='metric', name='observer.path_capacity_bytes', bytes=2),
            dict(kind='metric', name='allocator.retained_free', bytes=10)])
        self.assertEqual(result['unexplained_remainder_bytes'], 77)

    def test_global_shared_backing_first_owner(self):
        result = sample([owner(kind='backing', backing_id=123), owner('b', kind='backing', backing_id=123)])
        self.assertEqual(list(result['owners']), ['a'])
        self.assertEqual(result['owners']['a']['unique_backing_groups'], 1)
        self.assertEqual(result['unique_initialized_live_bytes'], 8)

    def test_inconsistent_backing_is_rejected(self):
        with self.assertRaises(ValueError):
            sample([owner(kind='backing', backing_id=123),
                    owner('b', kind='backing', backing_id=123, count=3)])

    def test_weighted_fill(self):
        result = sample([owner(), owner(non_default_entries=4, slots=8)])
        self.assertEqual(result['owners']['a']['fill_ratio'], .5)

    def test_zero_slots_and_unknown_are_not_zero_percent(self):
        result = sample([owner(slots=0, non_default_entries=0)])
        self.assertIsNone(result['owners']['a']['fill_ratio'])
        result = sample([owner(), owner(non_default_entries=None)])
        self.assertIsNone(result['owners']['a']['fill_ratio'])

    def test_unknown_capacity(self):
        result = sample([owner(capacity_bytes=None, unique_backing_bytes=None)])
        self.assertFalse(result['capacity_complete'])
        self.assertEqual(result['unique_initialized_live_bytes'], 8)

    def test_unknown_does_not_erase_known_capacity_fragments(self):
        result = sample([owner(), owner(capacity_bytes=None, unique_backing_bytes=None)])
        self.assertIsNone(result['owners']['a']['unique_backing_bytes'])
        self.assertEqual(result['known_unique_capacity_bytes'], 16)

    def test_negative_remainder_preserved(self):
        result = sample([owner(initialized_bytes=200, capacity_bytes=200)])
        self.assertEqual(result['remainder_before_allocator_retention_bytes'], -105)

    def test_duplicate_properties_deduplicate(self):
        rows = [dict(kind='duplicate_properties', backing_id=7, bytes=12)] * 2
        result = sample(rows)
        self.assertEqual(result['duplicate_properties_unique_lists'], 1)
        self.assertEqual(result['duplicate_properties_unique_payload_bytes'], 12)

    def test_diff_added_removed_unknown(self):
        result = m.diff([sample([owner('removed')])], [sample([owner('added', capacity_bytes=None)])])[0]
        self.assertEqual(result['owners']['removed']['initialized_bytes'], -8)
        self.assertEqual(result['owners']['added']['initialized_bytes'], 8)
        self.assertIsNone(result['owners']['added']['capacity_bytes'])
        self.assertEqual(result['initialized_live_delta'], 0)
        self.assertEqual(result['metrics_delta']['os.physical_footprint'], 0)
        self.assertIsNone(result['metrics_delta']['allocator.retained_free'])

    def test_diff_mismatched_instants(self):
        with self.assertRaises(ValueError):
            m.diff([sample([], 'after_load')], [sample([], 'after_check')])

    def test_complete_stream_and_pointer_reset(self):
        rows = [dict(kind='begin', schema=1, instant='after_load'), owner(kind='backing', backing_id=1),
                dict(kind='end', instant='after_load', write_failed=False)]
        results = m.reduce_lines(map(json.dumps, rows * 2))
        self.assertEqual([s['unique_initialized_live_bytes'] for s in results], [8, 8])

    def test_truncation_and_write_failure(self):
        begin = dict(kind='begin', schema=1, instant='after_check')
        for rows in ([begin], [begin, dict(kind='end', instant='after_check', write_failed=True)]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                m.reduce_lines(map(json.dumps, rows))

    def test_negative_boolean_and_oversize_rejected(self):
        for row in [owner(count=-1), owner(count=True), owner(initialized_bytes=99)]:
            with self.subTest(row=row), self.assertRaises(ValueError):
                sample([row])


if __name__ == '__main__':
    unittest.main()
