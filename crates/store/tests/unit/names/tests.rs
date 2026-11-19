use super::*;

#[test]
fn name_hints_keep_text_lengths_scalar_validity_and_legacy_fallback() {
    assert_eq!(std::mem::size_of::<NameRecord>(), 8);
    for value in ["", "ordinary", "名前", "a long identifier name"] {
        let id = push_str_with_contextual_hint(value, false);
        assert_eq!(text(id), value.as_bytes());
        assert_eq!(scalar_text(id), Some(value));
        assert!(!may_be_contextual_identifier(id));
    }
    let id = push_str_with_contextual_hint("private", true);
    assert!(may_be_contextual_identifier(id));
    assert_eq!(scalar_text(id), Some("private"));
    assert!(may_be_contextual_identifier(push_str("legacy unknown")));
    let surrogate = [0xed, 0xa0, 0x80];
    let id = push_with_contextual_hint(&surrogate, false);
    assert_eq!(text(id), surrogate);
    assert_eq!(scalar_text(id), None);
    assert!(may_be_contextual_identifier(id));
}

/// A thread's names lie in runs of its own: the next name takes the next id
/// and the bytes behind the last text unless a run is used up, whatever the
/// other threads store meanwhile.
#[test]
fn a_thread_fills_its_own_runs_of_ids_and_text() {
    let fill = |tag: u8| {
        let text = [tag; 5];
        // More than a run of ids and more than a run of text.
        let ids: Vec<u32> = (0..2000).map(|_| push(&text)).collect();
        for pair in ids.windows(2) {
            let (previous, next) = (
                crate::get::<NameRecord>(pair[0]),
                crate::get::<NameRecord>(pair[1]),
            );
            // The run the thread started with may be one an ended thread
            // left; every run after it starts at the cursor.
            assert!(pair[1] == pair[0] + 1 || pair[1].is_multiple_of(ID_RUN));
            assert!(
                next.start == previous.start + 5
                    || (next.start as usize).is_multiple_of(TEXT_ALIGN),
                "a text outside its thread's run"
            );
        }
        ids
    };
    let (first, second) = std::thread::scope(|scope| {
        let first = scope.spawn(|| fill(b'a'));
        let second = scope.spawn(|| fill(b'b'));
        (first.join().unwrap(), second.join().unwrap())
    });
    assert!(first.iter().all(|&id| text(id) == b"aaaaa"));
    assert!(second.iter().all(|&id| text(id) == b"bbbbb"));
    assert!(first.iter().all(|id| !second.contains(id)));
}

/// The arithmetic of the runs, on a supply of its own. A thread's first name
/// starts a run of ids and a run of text; a text that fits the run lies in
/// it, however long; a long one that does not fit takes whole cache lines of
/// its own and leaves the run where it was; a short one that does not fit
/// starts a new run. A thread that ends hands its unfinished run on, and the
/// next thread's names continue it, ids and text.
#[test]
fn runs_are_filled_in_order_and_handed_on() {
    let supply = Supply::new();
    let left = |supply: &Supply| supply.left.lock().unwrap().len();

    let mut first = Run::NONE;
    assert_eq!(supply.claim(&mut first, 5), (0, 0));
    assert_eq!(supply.claim(&mut first, LONG_TEXT), (1, 5));
    assert_eq!(supply.claim(&mut first, TEXT_RUN), (2, TEXT_RUN));
    assert_eq!(supply.claim(&mut first, 5), (3, 5 + LONG_TEXT));

    // A thread beside it has runs of its own.
    let mut second = Run::NONE;
    assert_eq!(supply.claim(&mut second, 5), (ID_RUN, 2 * TEXT_RUN));

    // The first thread ends; the next thread continues its run and leaves
    // nothing for the one after it.
    supply.hand_on(first);
    assert_eq!(left(&supply), 1);
    let mut third = Run::NONE;
    assert_eq!(supply.claim(&mut third, 5), (4, 10 + LONG_TEXT));
    assert_eq!(left(&supply), 0);
    let mut fourth = Run::NONE;
    assert_eq!(supply.claim(&mut fourth, 5), (2 * ID_RUN, 3 * TEXT_RUN));

    // The ids of the continued run used up: a new run of ids, the text room
    // kept.
    for id in 5..ID_RUN {
        assert_eq!(supply.claim(&mut third, 0).0, id);
    }
    assert_eq!(supply.claim(&mut third, 5), (3 * ID_RUN, 15 + LONG_TEXT));

    // A short text that does not fit what is left starts a new run of text.
    let short = LONG_TEXT - 1;
    assert_eq!(supply.claim(&mut third, short).1, 20 + LONG_TEXT);
    assert_eq!(supply.claim(&mut third, short).1, 20 + LONG_TEXT + short);
    assert!(third.text_end - third.text < short);
    assert_eq!(supply.claim(&mut third, short).1, 4 * TEXT_RUN);
    assert_eq!(supply.claim(&mut third, 5).1, 4 * TEXT_RUN + short);

    // A thread that stored nothing, or used up what it took, hands nothing
    // on; text room alone is handed on and continued under new ids.
    supply.hand_on(Run::NONE);
    supply.hand_on(Run {
        id: ID_RUN,
        id_end: ID_RUN,
        text: 100,
        text_end: 100,
    });
    assert_eq!(left(&supply), 0);
    supply.hand_on(Run {
        id: ID_RUN,
        id_end: ID_RUN,
        text: 100,
        text_end: 228,
    });
    assert_eq!(left(&supply), 1);
    let mut fifth = Run::NONE;
    assert_eq!(supply.claim(&mut fifth, 5), (4 * ID_RUN, 100));
    assert_eq!(left(&supply), 0);
}

/// A process that starts a thread for every request keeps filling the runs
/// its ended threads left: the names of threads that follow each other take
/// ids that follow each other. (Each test runs on a thread of its own and
/// hands its run on like any thread, so a few of the steps continue another
/// test's run; without the hand-over none would follow its predecessor.)
#[test]
fn a_thread_that_ends_hands_its_run_to_the_next() {
    let ids: Vec<u32> = (0..1000)
        .map(|_| std::thread::spawn(|| push(b"request")).join().unwrap())
        .collect();
    assert!(ids.iter().all(|&id| text(id) == b"request"));
    let following = ids.windows(2).filter(|pair| pair[1] == pair[0] + 1).count();
    assert!(following >= 500, "only {following} of 999 names followed");
}
