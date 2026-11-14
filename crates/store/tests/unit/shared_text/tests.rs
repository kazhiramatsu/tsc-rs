use super::*;

#[test]
fn static_text_keeps_read_only_bytes_across_threads_and_drops() {
    const SOURCE: &str = "export const value = '😀';\n";
    static TEXT: StaticText<{ SOURCE.len() }> = StaticText::new(SOURCE);
    static EMPTY: StaticText<0> = StaticText::new("");
    for shared in [TEXT.shared(), EMPTY.shared()] {
        assert!(!shared.in_text_heap());
        assert!(shared.is_static());
        assert_eq!(shared.as_ptr(), shared.ptr.as_ptr());
        assert_eq!(shared.holders(), STATIC_COUNT);
        let expected = shared.as_str().to_owned();
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let shared = shared.clone();
                let expected = &expected;
                scope.spawn(move || {
                    for _ in 0..100 {
                        let clone = shared.clone();
                        assert!(clone.ptr_eq(&shared));
                        assert_eq!(clone.as_str(), expected);
                    }
                });
            }
        });
        assert_eq!(shared.holders(), STATIC_COUNT);
        assert_eq!(shared, SharedText::new(&expected));
    }
    assert_eq!(TEXT.shared().as_ptr(), SOURCE.as_ptr());
    assert_eq!(TEXT.text, SOURCE);
    assert_eq!(EMPTY.text, "");
    assert_eq!(std::mem::size_of::<StaticText<0>>(), 16);
    assert_eq!(std::mem::size_of::<SharedText>(), 16);
    assert_eq!(std::mem::size_of::<Option<SharedText>>(), 16);
}

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
    let mut starts: Vec<usize> = texts.iter().map(|text| text.as_ptr().addr()).collect();
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
fn a_text_is_one_block_whose_holders_are_counted() {
    let text = SharedText::new("counted text");
    assert_eq!(text.holders(), 1);
    // The bytes follow the count in the one block.
    assert_eq!(text.as_ptr().addr() - text.block().as_ptr().addr(), HEADER);
    let clone = text.clone();
    assert_eq!(text.holders(), 2);
    drop(clone);
    assert_eq!(text.holders(), 1);
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let text = text.clone();
            std::thread::spawn(move || {
                for _ in 0..1_000 {
                    let clone = text.clone();
                    assert_eq!(clone.as_str(), "counted text");
                }
                text
            })
        })
        .collect();
    let returned: Vec<SharedText> = handles
        .into_iter()
        .map(|handle| handle.join().expect("clone thread"))
        .collect();
    assert_eq!(text.holders(), 5);
    drop(returned);
    assert_eq!(text.holders(), 1);
    assert_eq!(text.as_str(), "counted text");
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

#[test]
fn direct_input_handles_size_changes_short_reads_and_decoder_fallback() {
    use std::io::{self, Read};
    struct ShortInput<'a> {
        bytes: &'a [u8],
        interrupt: bool,
    }
    impl Read for ShortInput<'_> {
        fn read(&mut self, destination: &mut [u8]) -> io::Result<usize> {
            if self.interrupt {
                self.interrupt = false;
                return Err(io::ErrorKind::Interrupted.into());
            }
            let count = destination.len().min(self.bytes.len()).min(7);
            destination[..count].copy_from_slice(&self.bytes[..count]);
            self.bytes = &self.bytes[count..];
            Ok(count)
        }
    }
    let long = "const astral = '😀';\n".repeat(10000);
    for bytes in [
        b"".as_slice(),
        b"x",
        long.as_bytes(),
        b"\xef\xbb\xbftext",
        b"\xff\xfeA\0",
        b"\xfe\xff\0A",
        b"bad\xff",
    ] {
        for hint in [0, 1, bytes.len(), bytes.len() + 15] {
            let mut reader = ShortInput {
                bytes,
                interrupt: true,
            };
            match read_text(&mut reader, hint).unwrap() {
                TextRead::Text(text) => {
                    assert_eq!(text.as_bytes(), bytes);
                    assert!(text.in_text_heap());
                    let clone = text.clone();
                    drop(text);
                    assert_eq!(clone.as_bytes(), bytes);
                }
                TextRead::Bytes(original) => {
                    assert_eq!(original, bytes);
                    assert!(
                        bytes.starts_with(&[0xef, 0xbb, 0xbf])
                            || std::str::from_utf8(bytes).is_err()
                    );
                }
            }
        }
    }
    struct Failure;
    impl Read for Failure {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    assert_eq!(
        read_text(&mut Failure, 40).err().unwrap().kind(),
        io::ErrorKind::PermissionDenied
    );
}
