use super::*;

#[test]
fn readers_and_parsers_move_each_buffer_after_exactly_one_read() {
    let slots = SourceReads::new(256);
    let calls: Vec<_> = (0..256).map(|_| AtomicUsize::new(0)).collect();
    let order: Vec<_> = (0..256).rev().collect();
    let read = |index: usize| {
        assert_eq!(calls[index].fetch_add(1, Ordering::Relaxed), 0);
        vec![index as u8; 37]
    };
    std::thread::scope(|scope| {
        for _ in 0..3 {
            scope.spawn(|| slots.prepare(&order, &read));
        }
        for index in 0..256 {
            assert_eq!(slots.take(index, || read(index)), vec![index as u8; 37]);
        }
    });
    assert!(calls.iter().all(|count| count.load(Ordering::Relaxed) == 1));
}

#[test]
fn a_reader_panic_releases_its_claim() {
    let slots = SourceReads::<()>::new(1);
    assert!(std::panic::catch_unwind(|| slots.prepare(&[0], &|_| panic!("host panic"))).is_err());
    assert!(std::panic::catch_unwind(|| slots.take(0, || ())).is_err());
}
