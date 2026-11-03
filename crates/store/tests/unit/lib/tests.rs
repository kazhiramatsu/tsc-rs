use std::sync::Arc;

use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
struct Record {
    value: u64,
    owned: Option<Arc<u64>>,
}

// Regions 10 and 11 hold no record type of the compiler: the tests' own.
impl StoreRecord for Record {
    const REGION: usize = 11;
}

impl StoreRecord for u32 {
    const REGION: usize = 10;
}

fn records(values: std::ops::Range<u64>) -> StoreVec<Record> {
    let mut list = StoreVec::new();
    for value in values {
        list.push(Record {
            value,
            owned: Some(Arc::new(value)),
        });
    }
    list
}

#[test]
fn a_published_list_is_read_by_position_and_by_slice() {
    let mut list = records(0..4);
    list.publish(1_000);
    assert!(list.is_published());
    assert_eq!(list.len(), 4);
    assert_eq!(get::<Record>(1_002).value, 2);
    assert_eq!(slice::<Record>(1_001, 2)[1].value, 2);
    assert_eq!(list[3].value, 3);
}

#[test]
fn a_dropped_list_drops_its_records_and_zeroes_its_slots() {
    let owned = Arc::new(7);
    let mut list = StoreVec::new();
    list.push(Record {
        value: 7,
        owned: Some(Arc::clone(&owned)),
    });
    list.publish(2_000);
    assert_eq!(Arc::strong_count(&owned), 2);
    drop(list);
    assert_eq!(Arc::strong_count(&owned), 1);
    assert_eq!(get::<Record>(2_000).value, 0);
    assert!(get::<Record>(2_000).owned.is_none());
}

#[test]
fn a_large_dropped_range_reads_as_zeros_up_to_both_edges() {
    let count = (RETURN_THRESHOLD / std::mem::size_of::<u32>() * 3) as u32;
    let mut list = StoreVec::<u32>::new();
    list.resize(count as usize, 9);
    let position = 3;
    list.publish(position);
    assert_eq!(*get::<u32>(position + count - 1), 9);
    drop(list);
    assert!(slice::<u32>(position, count)
        .iter()
        .all(|&value| value == 0));
}

/// The tests of one binary run in parallel and share each region: these
/// rooms lie above the 786,432 slots the large-range test publishes from
/// position 3, which it fills with 9 while it runs (a claim finds its slots
/// zero only when no other test holds them).
const ROOM: u32 = 8_000_000;

#[test]
fn a_list_built_in_claimed_room_publishes_in_place_or_moves_out() {
    let mut list = StoreVec::<u32>::claim_room(ROOM, 3);
    list.push(1);
    list.push(2);
    assert!(list.is_published());
    assert_eq!(*get::<u32>(ROOM + 1), 2);
    list.publish(ROOM);
    assert_eq!(list.capacity(), 2);
    drop(list);
    assert_eq!(*get::<u32>(ROOM + 1), 0);

    let mut list = StoreVec::<u32>::claim_room(ROOM + 10, 2);
    list.push(1);
    list.push(2);
    list.push(3);
    assert!(!list.is_published());
    assert_eq!(&*list, &[1, 2, 3]);
    assert_eq!(*get::<u32>(ROOM + 10), 0, "the slots it left are zeroed");
    list.publish(ROOM + 20);
    assert_eq!(*get::<u32>(ROOM + 22), 3);

    let mut list = StoreVec::<u32>::claim_room(ROOM + 30, 2);
    list.push(4);
    list.publish(ROOM + 40);
    assert_eq!(*get::<u32>(ROOM + 30), 0, "the room it left is zeroed");
    assert_eq!(*get::<u32>(ROOM + 40), 4);
}

#[test]
fn a_large_list_grows_through_a_mapping_of_its_own() {
    let count = MAPPED_THRESHOLD / std::mem::size_of::<Record>() + 5;
    let list = records(0..count as u64);
    assert_eq!(list.len(), count);
    assert_eq!(list[count - 1].value, count as u64 - 1);
    let copy = list.clone();
    drop(list);
    assert_eq!(copy[count - 1].value, count as u64 - 1);
    assert_eq!(Arc::strong_count(copy[0].owned.as_ref().unwrap()), 1);
}

#[test]
fn a_clone_of_a_published_list_is_a_heap_copy() {
    let mut list = records(0..2);
    list.publish(3_000);
    let mut copy = list.clone();
    assert!(!copy.is_published());
    assert_eq!(copy, list);
    copy.push(Record::default());
    copy[0].value = 5;
    assert_eq!(get::<Record>(3_000).value, 0);
    assert_eq!(list[0].value, 0);
}

#[test]
#[should_panic(expected = "a published store list is final")]
fn a_published_list_does_not_grow() {
    let mut list = records(0..1);
    list.publish(4_000);
    list.push(Record::default());
}

#[test]
fn published_lists_are_read_from_other_threads() {
    let mut list = records(0..3);
    list.publish(5_000);
    let list = Arc::new(list);
    let reader = Arc::clone(&list);
    let sum = std::thread::spawn(move || reader.iter().map(|record| record.value).sum::<u64>())
        .join()
        .unwrap();
    assert_eq!(sum, 3);
}

#[test]
fn a_small_list_of_two_stays_in_its_record() {
    let mut list = SmallList::new();
    list.push(1u32);
    list.push(2);
    assert_eq!(&*list, &[1, 2]);
    assert_eq!(list.heap_bytes(), 0);
    list[0] = 3;
    assert_eq!(&*list.clone(), &[3, 2]);
}

#[test]
fn a_longer_small_list_moves_to_the_heap_and_keeps_its_order() {
    let mut list = SmallList::new();
    for value in 0..9u32 {
        list.push(value);
    }
    assert_eq!(&*list, &[0, 1, 2, 3, 4, 5, 6, 7, 8]);
    assert!(list.heap_bytes() >= 9 * 4);
    let copy = list.clone();
    drop(list);
    assert_eq!(copy.len(), 9);
    assert_eq!(copy[8], 8);
}

#[test]
fn zero_bytes_are_an_empty_small_list() {
    let mut zeroed = std::mem::MaybeUninit::<SmallList<u32>>::zeroed();
    // SAFETY: the claim under test.
    let list = unsafe { zeroed.assume_init_mut() };
    assert!(list.is_empty());
    list.push(7);
    assert_eq!(&**list, &[7]);
}

#[test]
fn a_name_is_read_back_by_its_id() {
    let plain = names::push(b"identifier");
    let empty = names::push(b"");
    // `a` and a lone lead surrogate, U+D800 in WTF-8.
    let lone = names::push(&[0x61, 0xED, 0xA0, 0x80]);
    assert_ne!(plain, empty);
    assert_eq!(names::text(plain), b"identifier");
    assert_eq!(names::scalar_text(plain), Some("identifier"));
    assert_eq!(names::text(empty), b"");
    assert_eq!(names::scalar_text(empty), Some(""));
    assert_eq!(names::text(lone), &[0x61, 0xED, 0xA0, 0x80]);
    assert_eq!(names::scalar_text(lone), None);
    let (count, bytes) = names::usage();
    assert!(count >= 3);
    assert!(bytes >= 3 * 8 + 14);
}

/// A text is its length word and its bytes, four to a word; it reads back the
/// same from the words and, once published, from the store.
#[test]
fn texts_read_back_from_their_words_and_the_store() {
    use crate::texts::{published_text, push_text, text_in, text_words, TextWord};
    let texts: [&[u8]; 5] = [b"", b"a", b"abcd", b"hello, world", "\u{1F600}x".as_bytes()];
    let mut words = StoreVec::<TextWord>::new();
    let starts: Vec<usize> = texts
        .iter()
        .map(|text| push_text(&mut words, text))
        .collect();
    assert_eq!(
        words.len(),
        texts
            .iter()
            .map(|text| text_words(text.len()))
            .sum::<usize>()
    );
    for (text, &start) in texts.iter().zip(&starts) {
        assert_eq!(text_in(&words, start), *text);
    }
    const TEXTS: u32 = 9_000_000;
    words.publish(TEXTS);
    for (text, &start) in texts.iter().zip(&starts) {
        assert_eq!(published_text(TEXTS + start as u32), *text);
    }
}
