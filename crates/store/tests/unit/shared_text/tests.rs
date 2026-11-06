use super::*;

#[test]
fn a_heap_text_reads_back_as_written() {
    for text in ["", "a", "const value = '😀';\n", &"x".repeat(70_000)] {
        let shared = SharedText::in_text_heap(text);
        assert_eq!(shared.as_str(), text);
        assert_eq!(&*shared, text);
        assert_eq!(shared.len(), text.len());
        assert_eq!(format!("{shared:?}"), format!("{text:?}"));
    }
}

#[test]
fn clones_share_the_text_and_equality_is_by_content() {
    let heap = SharedText::in_text_heap("shared text");
    let clone = heap.clone();
    assert!(heap.ptr_eq(&clone));
    assert_eq!(heap, clone);
    // A text in the heap equals the same text shared as `Arc<str>`, though
    // the two are different allocations.
    let shared = SharedText::from(Arc::<str>::from("shared text"));
    assert!(!heap.ptr_eq(&shared));
    assert_eq!(heap, shared);
    assert_ne!(heap, SharedText::in_text_heap("other text"));
    // A wrapped `Arc<str>` keeps its allocation.
    let arc: Arc<str> = Arc::from("kept");
    let wrapped = SharedText::from(Arc::clone(&arc));
    assert!(std::ptr::eq(wrapped.as_str(), &*arc));
}

#[test]
fn a_heap_text_is_freed_on_another_thread() {
    // Texts are allocated by loader threads and dropped wherever the last
    // holder goes; the text heap is a first-class heap.
    let texts: Vec<SharedText> = (0..64)
        .map(|index| SharedText::in_text_heap(&format!("text {index} {}", "y".repeat(index * 97))))
        .collect();
    let expected: Vec<String> = texts.iter().map(|text| text.as_str().to_owned()).collect();
    let reader = std::thread::spawn(move || {
        let read: Vec<String> = texts.iter().map(|text| text.as_str().to_owned()).collect();
        drop(texts);
        read
    });
    assert_eq!(reader.join().expect("reader thread"), expected);
    // And allocated on several threads at once.
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            std::thread::spawn(move || {
                (0..256)
                    .map(|index| SharedText::in_text_heap(&format!("{worker}:{index}")))
                    .map(|text| text.as_str().to_owned())
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    for (worker, handle) in workers.into_iter().enumerate() {
        let read = handle.join().expect("worker thread");
        assert_eq!(read.len(), 256);
        assert_eq!(read[255], format!("{worker}:255"));
    }
}
