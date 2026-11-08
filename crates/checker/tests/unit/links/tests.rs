use super::*;

#[derive(Clone, Copy)]
struct Key(usize);

impl DenseKey for Key {
    fn dense_index(self) -> usize {
        self.0
    }
}

#[test]
fn paged_table_fills_a_region_and_keeps_records_stable() {
    let region_ids = 1 << REGION_SHIFT;
    let mut table = PagedTable::<Key, usize>::with_id_bound(region_ids);
    let directories = directory_state(&table);
    // An odd multiplier permutes every id, so record slots differ from ids.
    let id = |ordinal| (ordinal * 109 + 71) & (region_ids - 1);
    let first = Key(id(0));
    assert_eq!(table.get(first), None);
    assert_eq!(table.memory_usage().0, 0);
    assert_eq!(allocated_chunks(&table), 0);
    *table.slot(first) = 1;
    let address = std::ptr::from_ref(table.get(first).unwrap());
    for ordinal in 1..region_ids {
        let key = Key(id(ordinal));
        assert_eq!(table.get(key), None);
        *table.slot(key) = ordinal + 1;
        assert_eq!(allocated_chunks(&table), ordinal / CHUNK_RECORDS + 1);
    }
    assert_eq!(table.len, region_ids);
    assert_eq!(table.region_lens[0], 32_768);
    assert_eq!(table.memory_usage().0, region_ids);
    assert_eq!(std::ptr::from_ref(table.get(first).unwrap()), address);
    for ordinal in 0..region_ids {
        let key = Key(id(ordinal));
        assert_eq!(table.get(key), Some(&(ordinal + 1)));
        assert_eq!(*table.slot(key), ordinal + 1);
    }
    assert_eq!(table.len, region_ids);
    assert_eq!(table.get(Key(region_ids)), None);
    assert_eq!(directory_state(&table), directories);
}

#[test]
fn paged_table_interleaves_regions_without_aliasing() {
    let mut table = PagedTable::<Key, usize>::with_id_bound((7 << REGION_SHIFT) + 1);
    let directories = directory_state(&table);
    let regions = [3, 0, 1];
    // Reverse id order and interleave regions across multiple record chunks.
    for ordinal in 0..600 {
        for region in regions {
            let id = (region << REGION_SHIFT) + ((1 << REGION_SHIFT) - 1 - ordinal);
            *table.slot(Key(id)) = id + 1;
        }
    }
    let first = Key((3 << REGION_SHIFT) + (1 << REGION_SHIFT) - 1);
    let address = std::ptr::from_ref(table.get(first).unwrap());
    *table.slot(Key(7 << REGION_SHIFT)) = 42;
    assert_eq!(std::ptr::from_ref(table.get(first).unwrap()), address);
    for ordinal in 0..600 {
        for region in regions {
            let id = (region << REGION_SHIFT) + ((1 << REGION_SHIFT) - 1 - ordinal);
            assert_eq!(table.get(Key(id)), Some(&(id + 1)));
            assert_eq!(*table.slot(Key(id)), id + 1);
        }
    }
    assert_eq!(table.get(Key(2 << REGION_SHIFT)), None);
    assert_eq!(table.get(Key(7 << REGION_SHIFT)), Some(&42));
    assert_eq!(table.len, 1801);
    assert_eq!(
        allocated_chunks(&table),
        3 * 600usize.div_ceil(CHUNK_RECORDS) + 1
    );
    assert_eq!(directory_state(&table), directories);
}

#[test]
fn paged_table_far_pages_use_the_same_region_slots() {
    let far_id = DENSE_PAGES << PAGE_SHIFT;
    let next_region = far_id + (1 << REGION_SHIFT);
    let mut table = PagedTable::<Key, usize>::with_id_bound(next_region + 1);
    let directories = directory_state(&table);
    assert_eq!(table.get(Key(far_id)), None);
    assert!(table.far.is_empty());
    *table.slot(Key(0)) = 1;
    for offset in 0..300 {
        *table.slot(Key(far_id + offset)) = offset + 2;
    }
    *table.slot(Key(next_region)) = 999;
    for offset in 0..300 {
        assert_eq!(table.get(Key(far_id + offset)), Some(&(offset + 2)));
        assert_eq!(*table.slot(Key(far_id + offset)), offset + 2);
    }
    assert_eq!(table.get(Key(far_id + 300)), None);
    assert_eq!(table.get(Key(next_region)), Some(&999));
    assert_eq!(table.get(Key(0)), Some(&1));
    assert_eq!(table.index.len(), DENSE_PAGES);
    assert_eq!(table.far.len(), 3);
    assert_eq!(table.len, 302);
    assert_eq!(allocated_chunks(&table), 4);
    assert_eq!(directory_state(&table), directories);

    // The public default constructor still accepts keys without a bound.
    let mut unbounded = PagedTable::<Key, usize>::default();
    *unbounded.slot(Key(far_id)) = 7;
    *unbounded.slot(Key(0)) = 8;
    *unbounded.slot(Key(next_region)) = 9;
    assert_eq!(unbounded.get(Key(far_id)), Some(&7));
    assert_eq!(unbounded.get(Key(0)), Some(&8));
    assert_eq!(unbounded.get(Key(next_region)), Some(&9));
}

fn allocated_chunks(table: &PagedTable<Key, usize>) -> usize {
    table.chunks.iter().filter(|chunk| chunk.is_some()).count()
}

fn directory_state(table: &PagedTable<Key, usize>) -> [(usize, usize, usize); 3] {
    [
        (
            table.index.as_ptr() as usize,
            table.index.len(),
            table.index.capacity(),
        ),
        (
            table.chunks.as_ptr() as usize,
            table.chunks.len(),
            table.chunks.capacity(),
        ),
        (
            table.region_lens.as_ptr() as usize,
            table.region_lens.len(),
            table.region_lens.capacity(),
        ),
    ]
}
