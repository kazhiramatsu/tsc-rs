use super::*;
use std::collections::HashSet;

fn version(value: &str) -> DocumentVersion {
    DocumentVersion::new(Arc::<str>::from(value))
}

fn assert_index_matches_text(snapshot: &TextSnapshot) {
    let text = snapshot.text();
    let actual = snapshot.positions();
    let dense = PositionIndex::new_static(text);
    assert_eq!(actual.byte_len(), dense.byte_len());
    assert_eq!(actual.utf16_len(), dense.utf16_len());
    assert_eq!(
        actual.line_count(),
        dense.line_count(),
        "line count differs for {text:?}; actual starts={:?}, dense starts={:?}",
        (0..actual.line_count())
            .map(|line| actual.line_start_byte(line))
            .collect::<Vec<_>>(),
        (0..dense.line_count())
            .map(|line| dense.line_start_byte(line))
            .collect::<Vec<_>>()
    );
    for byte in 0..=actual.byte_len() {
        assert_eq!(
            actual.byte_to_utf16(byte),
            dense.byte_to_utf16(byte),
            "byte conversion differs at {byte} in {text:?}"
        );
    }
    for utf16 in 0..=actual.utf16_len() {
        assert_eq!(
            actual.utf16_to_byte(utf16),
            dense.utf16_to_byte(utf16),
            "UTF-16 conversion differs at {utf16} in {text:?}"
        );
    }
    for line in 0..actual.line_count() {
        assert_eq!(actual.line_start_byte(line), dense.line_start_byte(line));
        assert_eq!(actual.line_start_utf16(line), dense.line_start_utf16(line));
    }
}

fn assert_change_preserves_unchanged_text(
    store: &VersionedTextStore,
    old: &TextSnapshot,
    new: &TextSnapshot,
) {
    let utf16 = store
        .utf16_change_range(old, new)
        .expect("retained ancestor has a UTF-16 change range");
    let old_start = old
        .positions()
        .utf16_to_byte(utf16.span.start)
        .expect("old change start is a scalar boundary") as usize;
    let old_end = old
        .positions()
        .utf16_to_byte(utf16.span.end().unwrap())
        .expect("old change end is a scalar boundary") as usize;
    let new_start = new
        .positions()
        .utf16_to_byte(utf16.span.start)
        .expect("new change start is a scalar boundary") as usize;
    let new_end = new
        .positions()
        .utf16_to_byte(utf16.span.start + utf16.new_length)
        .expect("new change end is a scalar boundary") as usize;
    assert_eq!(&old.text()[..old_start], &new.text()[..new_start]);
    assert_eq!(&old.text()[old_end..], &new.text()[new_end..]);

    let bytes = store
        .byte_change_range(old, new)
        .expect("retained ancestor has a byte change range");
    let old_start = bytes.span.start as usize;
    let old_end = bytes.span.end().unwrap() as usize;
    let new_start = bytes.span.start as usize;
    let new_end = (bytes.span.start + bytes.new_length) as usize;
    assert!(old.text().is_char_boundary(old_start));
    assert!(old.text().is_char_boundary(old_end));
    assert!(new.text().is_char_boundary(new_start));
    assert!(new.text().is_char_boundary(new_end));
    assert_eq!(&old.text()[..old_start], &new.text()[..new_start]);
    assert_eq!(&old.text()[old_end..], &new.text()[new_end..]);
}

#[test]
fn static_index_validates_boundaries_and_tsc_line_breaks() {
    let text = "a😀\r\nb\nc\u{2028}d\u{2029}e\u{0085}f";
    let index = PositionIndex::new_static(text);
    assert_eq!(index.kind(), PositionIndexKind::StaticDense);
    assert_eq!(index.byte_to_utf16(1), Some(1));
    assert_eq!(index.byte_to_utf16(2), None);
    assert_eq!(index.byte_to_utf16(5), Some(3));
    assert_eq!(index.utf16_to_byte(2), None);
    assert_eq!(index.utf16_to_byte(3), Some(5));
    let starts = (0..index.line_count())
        .map(|line| index.line_start_utf16(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(starts, [0, 5, 7, 9, 11]);
    assert_eq!(
        index.line_and_character_utf16(13),
        Some(LineAndCharacter {
            line: 4,
            character: 2,
        })
    );
}

#[test]
fn edited_index_is_persistent_and_materializes_exact_text() {
    let mut store = VersionedTextStore::new("first\nsecond\nthird", version("1"));
    let initial = store.current_snapshot();
    assert_eq!(initial.positions().kind(), PositionIndexKind::StaticDense);
    store
        .edit_utf16(Utf16TextSpan::new(6, 6), "middle😀", version("2"))
        .unwrap();
    let edited = store.snapshot();
    assert_eq!(edited.text(), "first\nmiddle😀\nthird");
    assert_eq!(
        edited.positions().kind(),
        PositionIndexKind::PersistentLines
    );
    assert_eq!(edited.positions().utf16_to_byte(12), Some(12));
    assert_eq!(edited.positions().utf16_to_byte(13), None);
    assert_eq!(edited.positions().utf16_to_byte(14), Some(16));
    assert_eq!(
        store.utf16_change_range(&initial, &edited),
        Some(Utf16TextChangeRange {
            span: Utf16TextSpan::new(6, 6),
            new_length: 8,
        })
    );
}

#[test]
fn edits_split_and_merge_every_supported_line_break_without_treating_nel_as_one() {
    let initial = "a\r\nb\rc\nd\u{2028}e\u{2029}f\u{0085}g";
    let mut store = VersionedTextStore::new(initial, version("1"));
    assert_index_matches_text(&store.current_snapshot());

    // Split CRLF, merge the following CR line, and add mixed breaks in a
    // single replacement. NEL deliberately remains inside its line.
    let crlf_lf = initial.find('\n').unwrap() as u32;
    store
        .edit_bytes(ByteTextSpan::new(crlf_lf, 1), "X\n\u{2028}", version("2"))
        .unwrap();
    let after_first = store.snapshot();
    assert_eq!(
        after_first.text(),
        "a\rX\n\u{2028}b\rc\nd\u{2028}e\u{2029}f\u{0085}g"
    );
    assert_index_matches_text(&after_first);

    let cr = after_first.text().find("b\r").unwrap() as u32 + 1;
    store
        .edit_bytes(ByteTextSpan::new(cr, 1), "", version("3"))
        .unwrap();
    let after_second = store.snapshot();
    assert_eq!(
        after_second.text(),
        "a\rX\n\u{2028}bc\nd\u{2028}e\u{2029}f\u{0085}g"
    );
    assert_index_matches_text(&after_second);
    assert_eq!(
        (0..after_second.positions().line_count())
            .map(|line| after_second.positions().line_start_byte(line).unwrap())
            .collect::<Vec<_>>(),
        compute_line_starts_in_both_units(after_second.text()).0
    );
}

fn node_identities(node: &Arc<LineNode>, identities: &mut HashSet<*const LineNode>) {
    identities.insert(Arc::as_ptr(node));
    if let Some(left) = &node.left {
        node_identities(left, identities);
    }
    if let Some(right) = &node.right {
        node_identities(right, identities);
    }
}

#[test]
fn a_local_edit_shares_untouched_persistent_subtrees() {
    let text = (0..64)
        .map(|line| format!("line-{line:02}\n"))
        .collect::<String>();
    let mut store = VersionedTextStore::new(text, version("1"));
    let old_root = Arc::clone(&store.working_tree.root);
    store
        .edit_utf16(Utf16TextSpan::new(8 * 32 + 5, 1), "X", version("2"))
        .unwrap();
    let new_root = Arc::clone(&store.working_tree.root);
    let mut old = HashSet::new();
    let mut new = HashSet::new();
    node_identities(&old_root, &mut old);
    node_identities(&new_root, &mut new);
    assert!(old.intersection(&new).count() > 16);
}

#[test]
fn edits_never_round_unicode_boundaries() {
    let mut store = VersionedTextStore::new("a😀b", version("same"));
    assert!(matches!(
        store.edit_utf16(Utf16TextSpan::new(2, 0), "x", version("same")),
        Err(TextEditError::InvalidScalarBoundary {
            unit: PositionUnit::Utf16,
            position: 2,
        })
    ));
    assert!(matches!(
        store.edit_bytes(ByteTextSpan::new(2, 0), "x", version("same")),
        Err(TextEditError::InvalidScalarBoundary {
            unit: PositionUnit::Byte,
            position: 2,
        })
    ));
}

#[test]
fn ninth_pending_or_over_256_utf16_edit_materializes() {
    let mut store = VersionedTextStore::new("", version("0"));
    for edit in 1..=8 {
        let outcome = store
            .edit_utf16(
                Utf16TextSpan::new((edit - 1) as u32, 0),
                "x",
                version(&edit.to_string()),
            )
            .unwrap();
        assert!(outcome.published_snapshot().is_none());
    }
    let ninth = store
        .edit_utf16(Utf16TextSpan::new(8, 0), "x", version("9"))
        .unwrap();
    assert!(ninth.published_snapshot().is_some());
    assert_eq!(store.pending_edit_count(), 0);

    let exactly_256 = "x".repeat(256);
    assert!(store
        .edit_utf16(Utf16TextSpan::new(9, 0), exactly_256, version("10"))
        .unwrap()
        .published_snapshot()
        .is_none());
    let over_256 = "😀".repeat(129);
    assert!(store
        .edit_utf16(Utf16TextSpan::new(265, 0), over_256, version("11"))
        .unwrap()
        .published_snapshot()
        .is_some());

    let mut deletion = VersionedTextStore::new("x".repeat(514), version("0"));
    assert!(deletion
        .edit_utf16(Utf16TextSpan::new(0, 256), "", version("1"))
        .unwrap()
        .published_snapshot()
        .is_none());
    assert!(deletion
        .edit_utf16(Utf16TextSpan::new(0, 257), "", version("2"))
        .unwrap()
        .published_snapshot()
        .is_some());
}

#[test]
fn history_is_bounded_and_host_versions_do_not_create_ancestry() {
    let mut store = VersionedTextStore::new("", version("equal"));
    let evicted = store.current_snapshot();
    let mut retained = Arc::clone(&evicted);
    for revision in 1..=8 {
        store
            .edit_utf16(
                Utf16TextSpan::new((revision - 1) as u32, 0),
                "x",
                version("equal"),
            )
            .unwrap();
        retained = store.snapshot();
    }
    assert_eq!(store.retained_snapshot_count(), 8);
    assert_eq!(store.utf16_change_range(&evicted, &retained), None);

    let other = VersionedTextStore::new("xxxxxxxx", version("equal"));
    let other_snapshot = other.current_snapshot();
    assert_eq!(store.utf16_change_range(&other_snapshot, &retained), None);
}

#[test]
fn sequential_change_ranges_collapse_like_typescript() {
    let changes = [
        Utf16TextChangeRange {
            span: Utf16TextSpan::new(10, 50),
            new_length: 30,
        },
        Utf16TextChangeRange {
            span: Utf16TextSpan::new(30, 30),
            new_length: 40,
        },
    ];
    assert_eq!(
        collapse_utf16_changes(&changes),
        Utf16TextChangeRange {
            span: Utf16TextSpan::new(10, 70),
            new_length: 60,
        }
    );
}

#[test]
fn deterministic_unicode_edit_stress_matches_a_flat_string_and_dense_index() {
    let mut expected = "alpha\r\nβ😀\ncharlie\u{2028}delta\u{2029}echo\u{0085}tail".to_owned();
    let mut store = VersionedTextStore::new(expected.clone(), version("same"));
    let mut published = vec![store.current_snapshot()];
    let insertions = [
        "",
        "x",
        "😀",
        "日本",
        "\n",
        "\r",
        "\r\n",
        "\u{2028}",
        "\u{2029}",
        "\u{0085}",
        "z😀\r\n終",
    ];
    let mut state = 0x5eed_1a01_cafe_f00du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };

    for edit in 0..1_000u32 {
        let boundaries = expected
            .char_indices()
            .map(|(position, _)| position)
            .chain(std::iter::once(expected.len()))
            .collect::<Vec<_>>();
        let mut start = boundaries[next() as usize % boundaries.len()];
        let mut end = boundaries[next() as usize % boundaries.len()];
        if start > end {
            std::mem::swap(&mut start, &mut end);
        }
        // Keep most edits local while still exercising whole-document
        // deletion and replacement at deterministic intervals.
        if edit % 97 != 0 && end.saturating_sub(start) > 12 {
            end = boundaries
                .iter()
                .copied()
                .find(|boundary| *boundary >= start.saturating_add(12))
                .unwrap_or(end)
                .min(end);
        }
        let inserted = insertions[next() as usize % insertions.len()];
        let document_version = if edit % 3 == 0 {
            version("same")
        } else {
            version(&edit.to_string())
        };
        let outcome = if edit % 2 == 0 {
            store.edit_bytes(
                ByteTextSpan::new(start as u32, (end - start) as u32),
                inserted,
                document_version,
            )
        } else {
            let utf16_start = expected[..start].encode_utf16().count() as u32;
            let utf16_end = expected[..end].encode_utf16().count() as u32;
            store.edit_utf16(
                Utf16TextSpan::new(utf16_start, utf16_end - utf16_start),
                inserted,
                document_version,
            )
        }
        .unwrap();
        expected.replace_range(start..end, inserted);

        let should_observe = outcome.published_snapshot().is_some() || edit % 7 == 0;
        if should_observe {
            let snapshot = store.snapshot();
            assert_eq!(snapshot.text(), expected);
            assert_eq!(
                snapshot.positions().kind(),
                PositionIndexKind::PersistentLines
            );
            assert_index_matches_text(&snapshot);
            if let Some(old) = published.last() {
                assert_change_preserves_unchanged_text(&store, old, &snapshot);
            }
            published.push(snapshot);
            if published.len() > MAX_RETAINED_SNAPSHOTS {
                published.remove(0);
            }
            assert!(store.retained_snapshot_count() <= MAX_RETAINED_SNAPSHOTS);
        }
    }

    let final_snapshot = store.snapshot();
    assert_eq!(final_snapshot.text(), expected);
    assert_index_matches_text(&final_snapshot);
}

#[test]
fn invalid_ranges_fail_without_mutating_the_working_text() {
    let mut store = VersionedTextStore::new("a😀b", version("1"));
    let before = store.current_snapshot();
    assert!(matches!(
        store.edit_bytes(ByteTextSpan::new(u32::MAX, 2), "x", version("2")),
        Err(TextEditError::RangeOverflow {
            unit: PositionUnit::Byte
        })
    ));
    assert!(matches!(
        store.edit_utf16(Utf16TextSpan::new(4, 1), "x", version("2")),
        Err(TextEditError::PositionOutOfBounds {
            unit: PositionUnit::Utf16,
            ..
        })
    ));
    assert_eq!(store.snapshot().text(), before.text());
    assert_eq!(store.pending_edit_count(), 0);
}
