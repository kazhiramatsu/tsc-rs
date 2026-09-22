use super::*;
use tsc_diagnostics::{gen, DiagnosticCategory, JsString, MessageChain, RelatedInfo};

fn row(start: u32) -> Diagnostic {
    Diagnostic::new(
        Some("a.ts".into()),
        Some(start),
        Some(1),
        MessageChain::new(&gen::Duplicate_identifier_0, &["name".to_owned()]),
    )
}

fn indexed_sink() -> DiagnosticSink {
    let mut sink = DiagnosticSink::default();
    for start in 0..64 {
        assert_eq!(sink.insert_unique(row(start)), start as usize);
    }
    assert!(sink.index.is_some());
    sink
}

#[test]
fn full_equality_and_keep_first_survive_edits_and_direct_duplicates() {
    let mut sink = indexed_sink();
    let original = row(0);
    sink.push(original.clone());
    assert_eq!(sink.insert_unique(original.clone()), 0);
    sink.update(0, |diagnostic| {
        diagnostic.message.category = DiagnosticCategory::Suggestion;
        diagnostic.related.push(RelatedInfo {
            file_name: None,
            start: None,
            length: None,
            message: original.message.clone(),
        });
    });
    assert!(sink.index.is_some());
    assert_eq!(sink.insert_unique(original), 64);
    assert_eq!(sink.insert_unique(sink[0].clone()), 0);

    // All of these share a bucket but must remain distinct full values.
    let mut variants = Vec::new();
    for text in [
        JsString::from_code_units(&[0xd800]),
        JsString::from_code_units(&[0xd801]),
    ] {
        let mut diagnostic = row(0);
        diagnostic.message.text = text;
        variants.push(diagnostic);
    }
    let mut diagnostic = row(0);
    diagnostic.related_information_present = true;
    variants.push(diagnostic);
    let mut diagnostic = row(0);
    diagnostic.skipped_on_no_emit = true;
    variants.push(diagnostic);
    for diagnostic in variants {
        let position = sink.len();
        assert_eq!(sink.insert_unique(diagnostic.clone()), position);
        assert_eq!(sink.insert_unique(diagnostic), position);
    }
}

#[test]
fn rollback_reuses_indices_and_retains_earlier_bucket_members() {
    let mut sink = indexed_sink();
    let checkpoint = sink.len();
    sink.push(row(0));
    let mut changed = row(0);
    changed.message.text = "another name".into();
    assert_eq!(sink.insert_unique(changed.clone()), checkpoint + 1);
    sink.truncate(checkpoint);
    assert!(sink.index.is_some());
    assert_eq!(sink.insert_unique(row(0)), 0);
    assert_eq!(sink.insert_unique(changed), checkpoint);
    assert_eq!(sink.insert_unique(row(65)), checkpoint + 1);
    sink.truncate(1000);
    assert_eq!(sink.len(), checkpoint + 2);
    sink.truncate(0);
    assert_eq!(sink.insert_unique(row(0)), 0);
}

#[test]
fn arbitrary_vec_mutation_and_panicking_edits_invalidate_the_index() {
    let mut sink = indexed_sink();
    sink[0].start = Some(200);
    assert!(sink.index.is_none());
    assert_eq!(sink.insert_unique(row(200)), 0);
    assert_eq!(sink.insert_unique(row(0)), 64);
    sink.update(1, |diagnostic| {
        diagnostic.file_name = Some("other.ts".into())
    });
    assert!(sink.index.is_none());
    assert_eq!(sink.insert_unique(row(1)), 65);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sink.update(2, |diagnostic| {
            diagnostic.start = Some(201);
            panic!("deliberate interrupted edit");
        });
    }));
    assert!(result.is_err());
    assert!(sink.index.is_none());
    assert_eq!(sink.insert_unique(row(201)), 2);

    sink.reverse();
    let expected = sink
        .iter()
        .position(|diagnostic| *diagnostic == row(0))
        .unwrap();
    assert_eq!(sink.insert_unique(row(0)), expected);
    sink.clear();
    assert_eq!(sink.insert_unique(row(0)), 0);
}

#[test]
fn mixed_operations_match_a_linear_ledger() {
    let mut sink = indexed_sink();
    let mut reference = sink.to_vec();
    let mut seed = 42_u64;
    for step in 0..4000 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let value = (seed >> 32) as usize;
        let diagnostic = row((value % 120) as u32);
        match step % 9 {
            0 => {
                sink.push(diagnostic.clone());
                reference.push(diagnostic);
            }
            1 if !reference.is_empty() => {
                let position = value % reference.len();
                sink.update(position, |row| {
                    row.message.category = DiagnosticCategory::Suggestion
                });
                reference[position].message.category = DiagnosticCategory::Suggestion;
            }
            3 if !reference.is_empty() => {
                let position = value % reference.len();
                sink[position] = diagnostic.clone();
                reference[position] = diagnostic;
            }
            2 if reference.len() > 64 => {
                let len = reference.len() - value % 5;
                sink.truncate(len);
                reference.truncate(len);
            }
            _ => {
                let expected = reference
                    .iter()
                    .position(|row| *row == diagnostic)
                    .unwrap_or_else(|| {
                        reference.push(diagnostic.clone());
                        reference.len() - 1
                    });
                assert_eq!(sink.insert_unique(diagnostic), expected, "step {step}");
            }
        }
        assert_eq!(&*sink, &reference, "step {step}");
    }
}
