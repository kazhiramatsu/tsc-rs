use super::*;

#[test]
fn a_text_lands_in_the_text_heap_and_reads_back_as_written() {
    for text in ["", "a", "const value = '😀';\n", &"x".repeat(70_000)] {
        let shared = SharedText::new(text);
        assert_eq!(shared.as_str(), text);
        assert_eq!(&*shared, text);
        assert_eq!(format!("{shared:?}"), format!("{text:?}"));
        assert!(shared.in_text_heap(), "{} bytes", text.len());
    }
    // Every producer copies in: an `Arc<str>` and a `String` alike.
    assert!(SharedText::from(Arc::<str>::from("from an Arc")).in_text_heap());
    assert!(SharedText::from(String::from("from a String")).in_text_heap());
}

#[test]
fn empty_texts_start_at_addresses_of_their_own() {
    // The printer's token-start memo and the source map's fast path key on a
    // text's address: no two live texts may start at the same one.
    let texts: Vec<SharedText> = (0..8).map(|_| SharedText::new("")).collect();
    let mut starts: Vec<usize> = texts.iter().map(|text| text.as_ptr() as usize).collect();
    starts.sort_unstable();
    starts.dedup();
    assert_eq!(starts.len(), texts.len());
    assert!(texts.iter().all(|text| text.is_empty()));
}

#[test]
fn clones_share_the_block_and_equality_is_by_content() {
    let text = SharedText::new("shared text");
    let clone = text.clone();
    assert!(text.ptr_eq(&clone));
    assert_eq!(text.as_ptr(), clone.as_ptr());
    assert_eq!(text, clone);
    let copy = SharedText::from(Arc::<str>::from("shared text"));
    assert!(!text.ptr_eq(&copy));
    assert_eq!(text, copy);
    assert_ne!(text, SharedText::new("other text"));
}

#[test]
fn texts_are_allocated_and_freed_across_threads() {
    // The loader's workers allocate texts; the last holder drops them on
    // whatever thread it runs.
    let texts: Vec<SharedText> = (0..64)
        .map(|index| SharedText::new(&format!("text {index} {}", "y".repeat(index * 97))))
        .collect();
    let expected: Vec<String> = texts.iter().map(|text| text.as_str().to_owned()).collect();
    let reader = std::thread::spawn(move || {
        let read: Vec<String> = texts.iter().map(|text| text.as_str().to_owned()).collect();
        drop(texts);
        read
    });
    assert_eq!(reader.join().expect("reader thread"), expected);
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            std::thread::spawn(move || {
                (0..256)
                    .map(|index| SharedText::new(&format!("{worker}:{index}")))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let made: Vec<Vec<SharedText>> = workers
        .into_iter()
        .map(|handle| handle.join().expect("worker thread"))
        .collect();
    for (worker, texts) in made.iter().enumerate() {
        assert_eq!(texts.len(), 256);
        assert_eq!(texts[255].as_str(), format!("{worker}:255"));
        assert!(texts.iter().all(SharedText::in_text_heap));
    }
    drop(made);
}
