"""Run: python3 -I target/.../tools/test_alloc_census.py"""
import copy
import importlib.util
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('alloc_census', pathlib.Path(__file__).with_name('alloc_census.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


def fixture():
    header = dict(type='header', format='tsrs-alloc-census', version=1,
        alloc_census=True, phase_ledger=True, perf_counters=False, config_error=False,
        sampling_rate=1024, slot_count=64, overflow_slot=64, size_bins=9, alignment_bins=8,
        sample_capacity=16, pointer_capacity=16, sample_attempts=2, dropped_samples=0,
        table_losses=0, table_peak=2, slot_overflow_events=0,
        preinstall_unsampled_events=0, recursion_events=0, unwinder='test')
    values = dict.fromkeys(m.FIELDS, 0)
    values.update(alloc=2, realloc=1, dealloc=1, requested_bytes=96,
                  realloc_old_bytes=16, realloc_new_bytes=32, freed_bytes=48,
                  realloc_moved=1, realloc_copy_bytes=16)
    a = dict(type='block', thread=0, phase=9, size_bin=1, alignment_bin=3, live_peak=64, **values)
    b = dict(type='block', thread=1, phase=9, size_bin=1, alignment_bin=4, live_peak=80, **values)
    totals = dict(type='totals', level='block', live_requested_bytes=64, peak_live_requested_bytes=96,
                  **{key: 2 * value for key, value in values.items()})
    sample = dict(type='sample', sequence=0, lifetime=1, pcs=['0x1000', '0x2000'])
    return [header, a, b, totals,
        dict(type='site', level='logical', site='node.list', phase=3, length_bin=0, count=5, reserved_or_length=0, used_bytes=0),
        dict(type='site', level='logical', site='node.list', phase=3, length_bin=1, count=3, reserved_or_length=3, used_bytes=12),
        dict(type='site', level='backing', site='store.reservation', phase=3, length_bin=0, count=1, reserved_or_length=1<<40, used_bytes=0),
        dict(type='site', level='backing', site='frozen.room', phase=3, length_bin=0, count=2, reserved_or_length=4096, used_bytes=128),
        sample, dict(sample, sequence=1, lifetime=2),
        dict(type='observer', **dict.fromkeys(m.FIELDS, 0)),
        dict(type='recursive', **dict.fromkeys(m.FIELDS, 0)),
        dict(type='pre_exit', all_live_requested_bytes=64, all_peak_live_requested_bytes=96),
        dict(type='complete', version=1)]


class ReductionTests(unittest.TestCase):
    def test_totals_and_peaks(self):
        out = m.reduce_rows(fixture())
        self.assertEqual(out['totals']['block']['requested_bytes'], 192)
        self.assertEqual(out['totals']['block']['realloc_copy_bytes'], 32)
        self.assertEqual(out['phase_size_bins'][0]['count'], 6)
        self.assertEqual(out['phase_size_bins'][0]['live_peak'], 80)  # max, never 144
        self.assertEqual(out['top_sites'][0]['estimated_events'], 2048)
        self.assertEqual(out['totals']['logical']['events'], 8)
        self.assertEqual(out['totals']['logical']['elements'], 3)
        self.assertNotIn('reserved_bytes', out['totals']['backing'])
        self.assertEqual(out['lifetimes'], {'same_phase': 1, 'later_phase': 1})

    def test_saturation_never_extrapolates_a_prefix(self):
        rows = fixture()
        rows[0].update(sample_attempts=3, dropped_samples=1)
        out = m.reduce_rows(rows)
        self.assertFalse(out['sampling_estimates_valid'])
        self.assertTrue(out['losses']['sample_buffer_saturated'])
        self.assertIsNone(out['top_sites'][0]['estimated_events'])
        self.assertEqual(out['totals']['block']['alloc'], 4)

    def test_lost_lifetime_is_not_retained(self):
        rows = fixture()
        rows[0]['table_losses'] = 1
        [row for row in rows if row['type'] == 'sample'][1]['lifetime'] = 3
        out = m.reduce_rows(rows)
        self.assertEqual(out['lifetimes']['tracking_lost'], 1)
        self.assertNotIn('never_freed_before_pre_exit', out['lifetimes'])

    def test_configured_sampling_rate(self):
        rows = fixture()
        rows[0]['sampling_rate'] = 8
        self.assertEqual(m.reduce_rows(rows)['top_sites'][0]['estimated_events'], 16)

    def test_no_stack_does_not_invent_site(self):
        rows = fixture()
        for row in rows:
            if row['type'] == 'sample':
                row['pcs'] = []
        out = m.reduce_rows(rows)
        self.assertEqual(len(out['top_sites']), 1)
        self.assertFalse(out['top_sites'][0]['stack_available'])
        self.assertIsNone(out['top_sites'][0]['estimated_events'])

    def test_symbols_json_and_nm_with_slide(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'symbols'
            path.write_text('{"slide":"0x1000","symbols":{"0x1000":"producer"}}')
            self.assertEqual(m.read_symbols(path)('0x2010'), 'producer + 0x10')
            path.write_text('0000000000001000 T producer\n')
            self.assertEqual(m.read_symbols(path)('0x1010'), 'producer + 0x10')

    def test_tagged_fallback_and_all_category_totals(self):
        rows = fixture()
        for row in rows:
            if row['type'] == 'sample':
                row.update(site='node.list', pcs=[])
            elif row['type'] == 'observer':
                row.update(alloc=3, requested_bytes=24)
        out = m.reduce_rows(rows)
        self.assertEqual(out['top_sites'][0]['explicit_site'], 'node.list')
        self.assertEqual(out['top_sites'][0]['estimated_events'], 2048)
        self.assertFalse(out['top_sites'][0]['stack_available'])
        self.assertEqual(out['totals']['block']['all_categories']['alloc'], 7)
        self.assertEqual(out['totals']['block']['all_categories_live']['all_peak_live_requested_bytes'], 96)

    def test_bad_headers(self):
        changes = [dict(version=2), dict(version=True), dict(format='other'), dict(sampling_rate=0),
            dict(sampling_rate=3), dict(slot_count=0), dict(config_error=0), dict(alloc_census=False),
            dict(size_bins=8), dict(table_peak=17), dict(sample_attempts=20)]
        for change in changes:
            with self.subTest(change=change):
                rows = fixture()
                rows[0].update(change)
                with self.assertRaises(ValueError):
                    m.reduce_rows(rows)
        with self.assertRaises(ValueError):
            m.reduce_rows([])

    def test_truncation_and_arithmetic_mismatch(self):
        with self.assertRaises(ValueError):
            m.reduce_rows(fixture()[:-1])
        rows = fixture()
        rows[3]['alloc'] += 1
        with self.assertRaises(ValueError):
            m.reduce_rows(rows)

    def test_invalid_rows(self):
        for change in (dict(size_bin=9), dict(phase=20), dict(alloc=-1), dict(live_peak=-1)):
            rows = fixture()
            rows[1].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                m.reduce_rows(rows)
        for key, value in [('pcs', ['symbol']), ('sequence', 0), ('lifetime', 5)]:
            rows = fixture()
            [row for row in rows if row['type'] == 'sample'][1][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                m.reduce_rows(rows)

    def test_duplicates_and_trailing_records(self):
        for index in (1, 4, -2):
            rows = fixture()
            rows.insert(1, copy.deepcopy(rows[index]))
            with self.subTest(index=index), self.assertRaises(ValueError):
                m.reduce_rows(rows)
        rows = fixture()
        rows.append(dict(type='perf', name='extra', value=1))
        with self.assertRaises(ValueError):
            m.reduce_rows(rows)


def peak_fixture():
    rows = fixture()
    rows[0].update(peak_schema=1, unwinder='darwin-arm64-frame-pointer-v1', unwind_empty_samples=0)
    rows.insert(1, dict(type='peak', instant='after_check', snapshots=1, serial=True,
                        sample_limit=2, live_samples=1, live_requested_bytes=8192))
    samples = [row for row in rows if row['type'] == 'sample']
    samples[0].update(peak_state=1, size=4096, phase=9, span=1, kind=0, success=True, end_kind=3, end_released=True)
    # Both are eventually freed: only the checkpoint state defines peak live.
    samples[1].update(peak_state=2, size=1024, phase=2, span=2, kind=2, success=True, end_kind=3, end_released=True)
    return rows


class PeakTests(unittest.TestCase):
    def test_live_and_freed_bytes_use_frozen_state_and_requested_size(self):
        out = m.reduce_rows(peak_fixture())
        self.assertTrue(out['peak_estimates_valid'])
        self.assertEqual(out['peak_by_stack']['live'][0]['estimated_bytes'], 4096 * 1024)
        self.assertEqual(out['peak_by_stack']['freed_before_peak'][0]['estimated_bytes'], 1024 * 1024)
        self.assertEqual(out['peak_by_stack']['live'][0]['phases'], {'9': 4096})
        self.assertEqual(out['peak_by_requested_size']['live'][0]['requested_size'], 4096)

    def test_in_place_realloc_is_separate_from_actual_block_release(self):
        rows = peak_fixture()
        sample = [r for r in rows if r['type'] == 'sample'][1]
        sample.update(end_kind=2, end_released=False)
        out = m.reduce_rows(rows)
        self.assertTrue(out['peak_estimates_valid'])
        self.assertEqual(out['peak_by_stack']['freed_before_peak'], [])
        self.assertEqual(out['peak_by_stack']['reallocated_in_place_before_peak'][0]['estimated_bytes'], 1024 * 1024)
        sample['end_released'] = True
        out = m.reduce_rows(rows)
        self.assertEqual(out['peak_by_stack']['reallocated_in_place_before_peak'], [])
        self.assertEqual(out['peak_by_stack']['freed_before_peak'][0]['estimated_bytes'], 1024 * 1024)
        sample.update(end_kind=3, end_released=False)
        with self.assertRaises(ValueError):
            m.reduce_rows(rows)

    def test_suppression_for_every_collection_loss(self):
        for change in [dict(dropped_samples=1, sample_attempts=3), dict(config_error=True),
                       dict(table_losses=1), dict(unwind_empty_samples=1), dict(unwinder='unsupported'),
                       dict(slot_overflow_events=1)]:
            rows = peak_fixture()
            rows[0].update(change)
            out = m.reduce_rows(rows)
            with self.subTest(change=change):
                self.assertFalse(out['peak_estimates_valid'])
                self.assertIsNone(out['peak_by_stack']['live'][0]['estimated_bytes'])
        for change in [dict(snapshots=2), dict(serial=False)]:
            rows = peak_fixture()
            rows[1].update(change)
            self.assertFalse(m.reduce_rows(rows)['peak_estimates_valid'])

    def test_rejects_missing_duplicate_inconsistent_or_malformed_checkpoint(self):
        for change in [dict(live_samples=2), dict(sample_limit=1), dict(sample_limit=20),
                       dict(instant='pre_exit'), dict(serial=1), dict(snapshots=-1)]:
            rows = peak_fixture()
            rows[1].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                m.reduce_rows(rows)
        rows = peak_fixture()
        del rows[1]
        with self.assertRaises(ValueError):
            m.reduce_rows(rows)
        rows = peak_fixture()
        rows.insert(1, copy.deepcopy(rows[1]))
        with self.assertRaises(ValueError):
            m.reduce_rows(rows)
        for change in [dict(peak_state=5), dict(peak_state=2, lifetime=0),
                       dict(peak_state=3), dict(success=False), dict(size=-1),
                       dict(phase=20), dict(kind=3), dict(peak_state=0)]:
            rows = peak_fixture()
            next(row for row in rows if row['type'] == 'sample').update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                m.reduce_rows(rows)

    def test_samples_after_checkpoint_cannot_be_live_at_peak(self):
        rows = peak_fixture()
        rows[1].update(sample_limit=1)
        samples = [row for row in rows if row['type'] == 'sample']
        samples[1]['peak_state'] = 0
        out = m.reduce_rows(rows)
        self.assertEqual(out['peak_by_stack']['freed_before_peak'], [])

    def test_peak_metadata_rejects_invalid_unwind_counts(self):
        for value in [False, -1, '0']:
            rows = peak_fixture()
            rows[0]['unwind_empty_samples'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                m.reduce_rows(rows)

    def test_missing_frames_never_qualify_stack_estimates(self):
        rows = peak_fixture()
        next(row for row in rows if row['type'] == 'sample')['pcs'] = []
        self.assertFalse(m.reduce_rows(rows)['peak_estimates_valid'])

    def test_exact_atos_maps_never_interpolate_into_another_image(self):
        with tempfile.TemporaryDirectory() as directory:
            path = pathlib.Path(directory) / 'symbols'
            path.write_text('{"exact":true,"symbols":{"0x1000":"producer"}}')
            lookup = m.read_symbols(path)
            self.assertEqual(lookup('0x1000'), 'producer')
            self.assertEqual(lookup('0x1004'), '0x1004')

    def test_stack_grouping_sums_bytes_and_keeps_phases(self):
        rows = peak_fixture()
        samples = [row for row in rows if row['type'] == 'sample']
        samples[1]['peak_state'] = 1
        rows[1]['live_samples'] = 2
        out = m.reduce_rows(rows)
        self.assertEqual(len(out['peak_by_stack']['live']), 1)
        self.assertEqual(out['peak_by_stack']['live'][0]['sampled_bytes'], 5120)
        self.assertEqual(out['peak_by_stack']['live'][0]['phases'], {'2': 1024, '9': 4096})


if __name__ == '__main__':
    unittest.main()
